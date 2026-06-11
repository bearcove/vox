use std::future::IntoFuture;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(any(
    feature = "transport-tcp",
    feature = "transport-local",
    feature = "transport-websocket"
))]
use vox_core::initiator;
use vox_core::{ConnectionRequest, ConnectionRouter, FromVoxSession, NoopClient, SessionError};
use vox_types::{
    DEFAULT_INITIAL_CHANNEL_CREDIT, Link, MaybeSend, MaybeSync, Metadata, VoxObserver,
    VoxObserverHandle, metadata_into_owned,
};

mod error;
pub use error::ServeError;

#[cfg(feature = "transport-tcp")]
mod tcp;

#[cfg(feature = "transport-local")]
mod local;

// Server-side: ws/wss listeners use tokio::net::TcpListener and only make
// sense on native targets.
#[cfg(all(feature = "transport-websocket", not(target_arch = "wasm32")))]
mod ws;
#[cfg(all(feature = "transport-websocket", not(target_arch = "wasm32")))]
pub use ws::WsListener;

#[cfg(all(feature = "transport-websocket-tls", not(target_arch = "wasm32")))]
mod wss;
#[cfg(all(feature = "transport-websocket-tls", not(target_arch = "wasm32")))]
pub use wss::WssListener;

mod channel;
pub use channel::{ChannelListener, ChannelListenerSender};

/// A listener that accepts incoming connections for [`serve_listener()`].
pub trait VoxListener: MaybeSend + 'static {
    /// The link type produced by this listener.
    type Link: Link + MaybeSend + 'static;

    /// Accept the next incoming connection.
    fn accept(
        &mut self,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Link>> + MaybeSend + '_;
}

#[cfg(not(target_arch = "wasm32"))]
type BoxHighLevelFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
#[cfg(target_arch = "wasm32")]
type BoxHighLevelFuture<T> = Pin<Box<dyn Future<Output = T>>>;

/// Connect to a remote vox service, returning a typed client.
///
/// The address string determines the transport:
///
/// - `tcp://host:port` or bare `host:port` — TCP stream transport
/// - `local://path` — Unix socket / Windows named pipe
/// - `ws://host:port/path` — WebSocket transport
///
/// # Examples
///
/// ```no_run
/// # #[vox::service]
/// # trait Hello {
/// #     async fn say_hello(&self) -> String;
/// # }
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client: HelloClient = vox::connect("127.0.0.1:9000").await?;
/// let reply = client.say_hello().await?;
/// # Ok(())
/// # }
/// ```
// r[impl rpc.session-setup]
pub fn connect<Client: FromVoxSession>(addr: impl std::fmt::Display) -> ConnectBuilder<Client> {
    ConnectBuilder::new(addr.to_string())
}

enum ConnectAddress {
    #[cfg(feature = "transport-tcp")]
    Tcp(String),
    #[cfg(feature = "transport-local")]
    Local(String),
    #[cfg(all(feature = "transport-websocket", not(target_arch = "wasm32")))]
    Ws(String),
}

fn parse_connect_address(addr: String) -> Result<ConnectAddress, SessionError> {
    let (scheme, host) = match addr.split_once("://") {
        Some((scheme, host)) => (scheme.to_string(), host.to_string()),
        None => ("tcp".to_string(), addr),
    };
    #[cfg(not(any(
        feature = "transport-tcp",
        feature = "transport-local",
        feature = "transport-websocket"
    )))]
    let _ = &host;

    match scheme.as_str() {
        #[cfg(feature = "transport-tcp")]
        "tcp" => Ok(ConnectAddress::Tcp(host)),
        #[cfg(feature = "transport-local")]
        "local" => Ok(ConnectAddress::Local(host)),
        #[cfg(all(feature = "transport-websocket", not(target_arch = "wasm32")))]
        "ws" | "wss" => Ok(ConnectAddress::Ws(format!("{scheme}://{host}"))),
        _ => Err(SessionError::Protocol(format!(
            "unknown transport scheme: {scheme:?}"
        ))),
    }
}

pub struct ConnectBuilder<Client> {
    addr: String,
    metadata: Metadata,
    on_connection: Option<Arc<dyn ConnectionRouter>>,
    connect_timeout: Option<Duration>,
    channel_capacity: u32,
    observer: Option<VoxObserverHandle>,
    wait_for_service: Option<Duration>,
    _client: std::marker::PhantomData<Client>,
}

impl<Client> ConnectBuilder<Client> {
    fn new(addr: String) -> Self {
        Self {
            addr,
            metadata: vox_types::Metadata::default(),
            on_connection: None,
            connect_timeout: Some(Duration::from_secs(5)),
            channel_capacity: DEFAULT_INITIAL_CHANNEL_CREDIT,
            observer: None,
            wait_for_service: None,
            _client: std::marker::PhantomData,
        }
    }

    // r[impl rpc.virtual-connection.accept]
    pub fn on_connection(mut self, router: impl ConnectionRouter) -> Self {
        self.on_connection = Some(Arc::new(router));
        self
    }

    pub fn metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    // r[impl rpc.flow-control.credit.initial.high-level]
    pub fn channel_capacity(mut self, channel_capacity: u32) -> Self {
        self.channel_capacity = channel_capacity;
        self
    }

    // r[impl rpc.observability.runtime]
    pub fn observer(mut self, observer: impl VoxObserver) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }

    // r[impl rpc.observability.runtime]
    pub fn observer_handle(mut self, observer: VoxObserverHandle) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Wait for the service to become reachable until `timeout`.
    ///
    /// Only transient failures (I/O errors, connect timeouts) are attempted again.
    /// Protocol errors, schema incompatibilities, and explicit rejections fail
    /// immediately.
    pub fn wait_for_service(mut self, timeout: Duration) -> Self {
        self.wait_for_service = Some(timeout);
        self
    }
}

const INITIAL_CONNECT_BACKOFF_MIN: Duration = Duration::from_millis(100);
const INITIAL_CONNECT_BACKOFF_MAX: Duration = Duration::from_secs(5);
const CHANNEL_CAPACITY_ZERO_ERROR: &str = "channel_capacity must be greater than zero";

// r[impl rpc.flow-control.credit.initial.zero]
fn validate_channel_capacity(channel_capacity: u32) -> Result<(), SessionError> {
    if channel_capacity == 0 {
        return Err(SessionError::Protocol(CHANNEL_CAPACITY_ZERO_ERROR.into()));
    }
    Ok(())
}

impl<Client> ConnectBuilder<Client>
where
    Client: FromVoxSession,
{
    pub async fn establish(self) -> Result<Client, SessionError> {
        let ConnectBuilder {
            addr,
            metadata,
            on_connection,
            connect_timeout,
            channel_capacity,
            observer,
            wait_for_service,
            _client: _,
        } = self;
        validate_channel_capacity(channel_capacity)?;

        tracing::debug!(
            service = Client::SERVICE_NAME,
            %addr,
            channel_capacity,
            wait_for_service = wait_for_service.is_some(),
            "vox high-level connect starting"
        );
        let parsed = parse_connect_address(addr)?;
        let metadata = metadata_into_owned(metadata);

        match wait_for_service {
            Some(service_timeout) => {
                let deadline = Instant::now() + service_timeout;
                let mut backoff = INITIAL_CONNECT_BACKOFF_MIN;

                loop {
                    // Cap each attempt by the remaining waiting budget so a single
                    // slow attempt cannot exceed the caller-supplied timeout.
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(SessionError::ConnectTimeout);
                    }
                    let remaining = deadline - now;

                    let attempt = Self::establish_once(
                        &parsed,
                        metadata.clone(),
                        on_connection.clone(),
                        connect_timeout,
                        channel_capacity,
                        observer.clone(),
                    );
                    let result = match moire::time::timeout(remaining, attempt).await {
                        Ok(r) => r,
                        Err(_) => Err(SessionError::ConnectTimeout),
                    };

                    match result {
                        Ok(client) => {
                            tracing::debug!(
                                service = Client::SERVICE_NAME,
                                "vox high-level connect established"
                            );
                            return Ok(client);
                        }
                        Err(e)
                            if !matches!(e, SessionError::Io(_) | SessionError::ConnectTimeout) =>
                        {
                            return Err(e);
                        }
                        Err(e) => {
                            let now = Instant::now();
                            if now >= deadline {
                                return Err(e);
                            }
                            let remaining = deadline - now;
                            let sleep = backoff.min(remaining);
                            tracing::debug!(
                                service = Client::SERVICE_NAME,
                                error = ?e,
                                ?sleep,
                                "vox high-level connect attempt failed; backing off"
                            );
                            moire::time::sleep(sleep).await;
                            backoff = backoff.saturating_mul(2).min(INITIAL_CONNECT_BACKOFF_MAX);
                        }
                    }
                }
            }
            None => {
                let result = Self::establish_once(
                    &parsed,
                    metadata,
                    on_connection,
                    connect_timeout,
                    channel_capacity,
                    observer,
                )
                .await;
                match &result {
                    Ok(_) => tracing::debug!(
                        service = Client::SERVICE_NAME,
                        "vox high-level connect established"
                    ),
                    Err(error) => tracing::debug!(
                        service = Client::SERVICE_NAME,
                        ?error,
                        "vox high-level connect failed"
                    ),
                }
                result
            }
        }
    }

    async fn establish_once(
        parsed: &ConnectAddress,
        metadata: vox_types::Metadata,
        on_connection: Option<Arc<dyn ConnectionRouter>>,
        connect_timeout: Option<Duration>,
        channel_capacity: u32,
        observer: Option<VoxObserverHandle>,
    ) -> Result<Client, SessionError> {
        #[cfg(not(any(
            feature = "transport-tcp",
            feature = "transport-local",
            feature = "transport-websocket"
        )))]
        let _ = (
            &metadata,
            &on_connection,
            &connect_timeout,
            channel_capacity,
            &observer,
        );

        match parsed {
            #[cfg(feature = "transport-tcp")]
            ConnectAddress::Tcp(host) => {
                tracing::trace!(
                    service = Client::SERVICE_NAME,
                    transport = "tcp",
                    %host,
                    "vox high-level connect attempt"
                );
                let mut builder = initiator(vox_stream::tcp_link_source(host.clone()));
                if let Some(router) = on_connection.clone() {
                    builder = builder.on_connection(RouterRef(router));
                }
                if let Some(timeout) = connect_timeout {
                    builder = builder.connect_timeout(timeout);
                }
                builder = builder.channel_capacity(channel_capacity);
                if let Some(observer) = observer.clone() {
                    builder = builder.observer_handle(observer);
                }
                builder.metadata(metadata).establish::<Client>().await
            }
            #[cfg(feature = "transport-local")]
            ConnectAddress::Local(host) => {
                tracing::trace!(
                    service = Client::SERVICE_NAME,
                    transport = "local",
                    %host,
                    "vox high-level connect attempt"
                );
                let mut builder = initiator(vox_stream::local_link_source(host.clone()));
                if let Some(router) = on_connection.clone() {
                    builder = builder.on_connection(RouterRef(router));
                }
                if let Some(timeout) = connect_timeout {
                    builder = builder.connect_timeout(timeout);
                }
                builder = builder.channel_capacity(channel_capacity);
                if let Some(observer) = observer.clone() {
                    builder = builder.observer_handle(observer);
                }
                builder.metadata(metadata).establish::<Client>().await
            }
            // Native-only: `ws_link_source` is the tokio-tungstenite reconnect
            // source. Wasm clients use the lower-level vox_websocket::WsLink
            // (web_sys::WebSocket) directly.
            #[cfg(all(feature = "transport-websocket", not(target_arch = "wasm32")))]
            ConnectAddress::Ws(url) => {
                tracing::trace!(
                    service = Client::SERVICE_NAME,
                    transport = "ws",
                    %url,
                    "vox high-level connect attempt"
                );
                let mut builder = initiator(vox_websocket::ws_link_source(url.clone()));
                if let Some(router) = on_connection {
                    builder = builder.on_connection(RouterRef(router));
                }
                if let Some(timeout) = connect_timeout {
                    builder = builder.connect_timeout(timeout);
                }
                builder = builder.channel_capacity(channel_capacity);
                if let Some(observer) = observer {
                    builder = builder.observer_handle(observer);
                }
                builder.metadata(metadata).establish::<Client>().await
            }
            #[allow(unreachable_patterns)]
            _ => Err(SessionError::Protocol(
                "transport not enabled in this vox build".to_string(),
            )),
        }
    }
}

impl<Client> IntoFuture for ConnectBuilder<Client>
where
    Client: FromVoxSession + 'static,
{
    type Output = Result<Client, SessionError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + 'static>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.establish())
    }
}

/// Serve a vox service by address string, accepting connections in a loop.
///
/// The address string determines the transport:
///
/// - `tcp://host:port` or bare `host:port` — TCP stream transport
/// - `local://path` — Unix socket / Windows named pipe
/// - `ws://host:port` — WebSocket (accepts TCP, upgrades to WS)
/// - `wss://host:port?cert=/path/to/cert.pem&key=/path/to/key.pem` — WebSocket over TLS
///
/// This function runs forever (or until an I/O error occurs). Each incoming
/// connection is handled in a spawned task.
///
/// # Examples
///
/// ```no_run
/// # #[vox::service]
/// # trait Hello {
/// #     async fn say_hello(&self) -> String;
/// # }
/// # #[derive(Clone)]
/// # struct HelloService;
/// # impl Hello for HelloService {
/// #     async fn say_hello(&self) -> String { "hi".into() }
/// # }
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// vox::serve("0.0.0.0:9000", HelloDispatcher::new(HelloService)).await?;
/// # Ok(())
/// # }
/// ```
pub fn serve<R: ConnectionRouter>(addr: impl std::fmt::Display, router: R) -> ServeBuilder<R> {
    ServeBuilder::new(addr.to_string(), router)
}

pub struct ServeBuilder<R> {
    addr: String,
    router: R,
    channel_capacity: u32,
    observer: Option<VoxObserverHandle>,
}

impl<R> ServeBuilder<R> {
    fn new(addr: String, router: R) -> Self {
        Self {
            addr,
            router,
            channel_capacity: DEFAULT_INITIAL_CHANNEL_CREDIT,
            observer: None,
        }
    }

    // r[impl rpc.flow-control.credit.initial.high-level]
    pub fn channel_capacity(mut self, channel_capacity: u32) -> Self {
        self.channel_capacity = channel_capacity;
        self
    }

    // r[impl rpc.observability.runtime]
    pub fn observer(mut self, observer: impl VoxObserver) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }

    // r[impl rpc.observability.runtime]
    pub fn observer_handle(mut self, observer: VoxObserverHandle) -> Self {
        self.observer = Some(observer);
        self
    }
}

impl<R> ServeBuilder<R>
where
    R: ConnectionRouter,
{
    pub async fn run(self) -> Result<(), ServeError> {
        let Self {
            addr,
            router,
            channel_capacity,
            observer,
        } = self;
        validate_channel_capacity(channel_capacity)?;
        let (scheme, host) = match addr.split_once("://") {
            Some((scheme, host)) => (scheme.to_string(), host.to_string()),
            None => ("tcp".to_string(), addr),
        };
        #[cfg(not(any(
            feature = "transport-tcp",
            feature = "transport-local",
            feature = "transport-websocket",
            feature = "transport-websocket-tls"
        )))]
        let _ = (&host, &router, &observer);

        match scheme.as_str() {
            #[cfg(feature = "transport-tcp")]
            "tcp" => {
                let listener = tokio::net::TcpListener::bind(&host).await?;
                let mut builder =
                    serve_listener(listener, router).channel_capacity(channel_capacity);
                if let Some(observer) = observer {
                    builder = builder.observer_handle(observer);
                }
                Ok(builder.await?)
            }
            #[cfg(feature = "transport-local")]
            "local" => local::serve_local(&host, router, channel_capacity, observer).await,
            #[cfg(all(feature = "transport-websocket", not(target_arch = "wasm32")))]
            "ws" => {
                let listener = WsListener::bind(&host).await?;
                let mut builder =
                    serve_listener(listener, router).channel_capacity(channel_capacity);
                if let Some(observer) = observer {
                    builder = builder.observer_handle(observer);
                }
                Ok(builder.await?)
            }
            #[cfg(all(feature = "transport-websocket-tls", not(target_arch = "wasm32")))]
            "wss" => wss::serve_wss(&host, router, channel_capacity, observer).await,
            _ => Err(ServeError::UnsupportedScheme { scheme }),
        }
    }
}

impl<R> IntoFuture for ServeBuilder<R>
where
    R: ConnectionRouter,
{
    type Output = Result<(), ServeError>;
    type IntoFuture = BoxHighLevelFuture<Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

/// Serve a vox service on a pre-bound listener.
///
/// Takes a [`VoxListener`] (e.g. `TcpListener`) and a [`ConnectionRouter`].
/// Each incoming connection is handled in a spawned task. Runs until an I/O
/// error occurs on the listener.
///
/// # Examples
///
/// ```no_run
/// # #[vox::service]
/// # trait Hello {
/// #     async fn say_hello(&self) -> String;
/// # }
/// # #[derive(Clone)]
/// # struct HelloService;
/// # impl Hello for HelloService {
/// #     async fn say_hello(&self) -> String { "hi".into() }
/// # }
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let listener = tokio::net::TcpListener::bind("0.0.0.0:9000").await?;
/// vox::serve_listener(listener, HelloDispatcher::new(HelloService)).await?;
/// # Ok(())
/// # }
/// ```
pub fn serve_listener<L, R>(listener: L, router: R) -> ServeListenerBuilder<L, R>
where
    L: VoxListener,
    <L::Link as Link>::Tx: MaybeSend + MaybeSync + 'static,
    <L::Link as Link>::Rx: MaybeSend + 'static,
    R: ConnectionRouter,
{
    ServeListenerBuilder::new(listener, router)
}

pub struct ServeListenerBuilder<L, R> {
    listener: L,
    router: R,
    channel_capacity: u32,
    observer: Option<VoxObserverHandle>,
}

impl<L, R> ServeListenerBuilder<L, R> {
    fn new(listener: L, router: R) -> Self {
        Self {
            listener,
            router,
            channel_capacity: DEFAULT_INITIAL_CHANNEL_CREDIT,
            observer: None,
        }
    }

    // r[impl rpc.flow-control.credit.initial.high-level]
    pub fn channel_capacity(mut self, channel_capacity: u32) -> Self {
        self.channel_capacity = channel_capacity;
        self
    }

    // r[impl rpc.observability.runtime]
    pub fn observer(mut self, observer: impl VoxObserver) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }

    // r[impl rpc.observability.runtime]
    pub fn observer_handle(mut self, observer: VoxObserverHandle) -> Self {
        self.observer = Some(observer);
        self
    }
}

impl<L, R> ServeListenerBuilder<L, R>
where
    L: VoxListener,
    <L::Link as Link>::Tx: MaybeSend + MaybeSync + 'static,
    <L::Link as Link>::Rx: MaybeSend + 'static,
    R: ConnectionRouter,
{
    pub async fn run(mut self) -> Result<(), SessionError> {
        validate_channel_capacity(self.channel_capacity)?;
        let router: Arc<dyn ConnectionRouter> = Arc::new(self.router);
        loop {
            tracing::trace!("vox high-level listener waiting for connection");
            let link = self.listener.accept().await.map_err(SessionError::Io)?;
            tracing::debug!("vox high-level listener accepted raw connection");
            let router = router.clone();
            let observer = self.observer.clone();
            let channel_capacity = self.channel_capacity;
            moire::spawn(async move {
                tracing::trace!("vox high-level listener establishing root session");
                let mut builder = vox_core::acceptor_on(link)
                    .on_connection(RouterRef(router))
                    .channel_capacity(channel_capacity);
                if let Some(observer) = observer {
                    builder = builder.observer_handle(observer);
                }
                let result = builder.establish::<NoopClient>().await;
                match result {
                    Ok(client) => {
                        tracing::debug!("vox high-level listener established root session");
                        client.caller.closed().await;
                        tracing::debug!("vox high-level listener root session closed");
                    }
                    Err(error) => {
                        tracing::debug!(?error, "vox high-level listener root session failed");
                    }
                }
            });
        }
    }
}

impl<L, R> IntoFuture for ServeListenerBuilder<L, R>
where
    L: VoxListener,
    <L::Link as Link>::Tx: MaybeSend + MaybeSync + 'static,
    <L::Link as Link>::Rx: MaybeSend + 'static,
    R: ConnectionRouter,
{
    type Output = Result<(), SessionError>;
    type IntoFuture = BoxHighLevelFuture<Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

/// Wrapper that implements `ConnectionRouter` by delegating to an `Arc<dyn ConnectionRouter>`.
struct RouterRef(Arc<dyn ConnectionRouter>);

impl ConnectionRouter for RouterRef {
    fn route(&self, request: &ConnectionRequest) -> Result<vox_core::ConnectionRoute, Metadata> {
        self.0.route(request)
    }
}
