//! End-to-end tests for automatic service name routing via vox-service metadata.

use vox::{ConnectionSettings, Parity, memory_link_pair};

#[vox::service]
trait Echo {
    async fn echo(&self, value: u32) -> u32;
}

#[derive(Clone)]
struct EchoService;

impl Echo for EchoService {
    async fn echo(&self, value: u32) -> u32 {
        value
    }
}

#[vox::service]
trait Adder {
    async fn add(&self, a: u32, b: u32) -> u32;
}

#[derive(Clone)]
struct AdderService;

impl Adder for AdderService {
    async fn add(&self, a: u32, b: u32) -> u32 {
        a + b
    }
}

#[derive(Clone, Debug, PartialEq, Eq, facet::Facet)]
#[repr(u8)]
enum StatusState {
    Idle = 0,
    Running = 1,
}

#[derive(Clone, Debug, PartialEq, Eq, facet::Facet)]
pub struct StatusRun {
    id: u64,
    state: StatusState,
    target_pid: Option<u32>,
    label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, facet::Facet)]
pub struct StatusSnapshot {
    active: Vec<StatusRun>,
    history: Vec<StatusRun>,
    daemon_started_unix_ns: u64,
}

#[vox::service]
trait StatusProbe {
    async fn status(&self) -> StatusSnapshot;
}

#[derive(Clone)]
struct StatusProbeService;

impl StatusProbe for StatusProbeService {
    async fn status(&self) -> StatusSnapshot {
        StatusSnapshot {
            active: vec![StatusRun {
                id: 7,
                state: StatusState::Running,
                target_pid: Some(4242),
                label: "current".to_string(),
            }],
            history: vec![StatusRun {
                id: 3,
                state: StatusState::Idle,
                target_pid: None,
                label: "previous".to_string(),
            }],
            daemon_started_unix_ns: 123_456,
        }
    }
}

#[tokio::test]
async fn root_connect_sends_vox_service_and_factory_sees_it() {
    use std::sync::{Arc, Mutex};

    let (client_link, server_link) = memory_link_pair(16);
    let seen_service = Arc::new(Mutex::new(None::<String>));

    // Server uses a factory that records the service name it sees.
    let factory = vox::router_fn({
        let seen_service = seen_service.clone();
        move |request: &vox::ConnectionRequest| -> Result<vox::ConnectionRoute, vox::Metadata> {
            *seen_service.lock().unwrap() = Some(request.service().to_string());
            match request.service() {
                "Echo" => Ok(vox::ConnectionRoute::handle(EchoDispatcher::new(
                    EchoService,
                ))),
                "Noop" => Ok(vox::ConnectionRoute::handle(())),
                _ => Err(Default::default()),
            }
        }
    });

    let server = tokio::spawn(async move {
        vox::acceptor_on(server_link)
            .on_connection(factory)
            .establish::<vox::NoopClient>()
            .await
            .expect("server establish")
    });

    let root = vox::initiator_on(client_link)
        .establish::<vox::NoopClient>()
        .await
        .expect("client establish");

    let _server_guard = server.await.expect("server task");
    let session = root.session.clone().unwrap();

    // Open a typed Echo vconn — this triggers the factory
    let echo: EchoClient = session
        .open(ConnectionSettings {
            parity: Parity::Odd,
            max_concurrent_requests: 64,
            initial_channel_credit: 16,
        })
        .await
        .expect("open Echo vconn");

    // Verify the factory saw vox-service: "Echo"
    let service = seen_service.lock().unwrap().clone();
    assert_eq!(service.as_deref(), Some("Echo"));

    let result = echo.echo(42).await.expect("echo call");
    assert_eq!(result, 42);
}

// r[verify schema.exchange.required]
#[tokio::test]
async fn root_factory_typed_status_response_round_trips_schema() {
    let response_schema = vox_phon::schema_bytes::<Result<StatusSnapshot, vox::VoxError>>()
        .expect("response schema bytes");
    let response_bundle =
        vox_phon::parse_schema_bytes(&response_schema).expect("response schema bundle");
    vox_phon::build_decode_program::<Result<StatusSnapshot, vox::VoxError>>(&response_bundle)
        .expect("response schema should decode its own reader type");

    let (client_link, server_link) = memory_link_pair(16);

    let factory = vox::router_fn(
        |request: &vox::ConnectionRequest| -> Result<vox::ConnectionRoute, vox::Metadata> {
            match request.service() {
                "StatusProbe" => Ok(vox::ConnectionRoute::handle(StatusProbeDispatcher::new(
                    StatusProbeService,
                ))),
                "Noop" => Ok(vox::ConnectionRoute::handle(())),
                _ => Err(Default::default()),
            }
        },
    );

    let server = tokio::spawn(async move {
        vox::acceptor_on(server_link)
            .on_connection(factory)
            .establish::<vox::NoopClient>()
            .await
            .expect("server establish")
    });

    let root = vox::initiator_on(client_link)
        .establish::<vox::NoopClient>()
        .await
        .expect("client establish");

    let _server_guard = server.await.expect("server task");

    let session = root.session.clone().unwrap();
    let client: StatusProbeClient = session
        .open(ConnectionSettings {
            parity: Parity::Odd,
            max_concurrent_requests: 64,
            initial_channel_credit: 16,
        })
        .await
        .expect("open StatusProbe vconn");

    let method_id = status_probe_service_descriptor().methods[0].id;
    let args = ();
    let with_tracker = client
        .caller
        .call(vox::RequestCall {
            method_id,
            channels: Default::default(),
            metadata: Default::default(),
            args: vox::Payload::outgoing(&args),
            schemas: Default::default(),
        })
        .await
        .expect("raw status call");

    let writer_schema = with_tracker
        .tracker
        .writer_schema_bytes(method_id, vox::BindingDirection::Response)
        .expect("status response schema binding");
    let writer_bundle =
        vox_phon::parse_schema_bytes(&writer_schema).expect("status response schema bundle");
    assert_eq!(
        writer_bundle.root, response_bundle.root,
        "status response schema root should be the full Result<StatusSnapshot, VoxError> wire shape"
    );

    let response = with_tracker.value;
    let ret_bytes = match &response.get().ret {
        vox::Payload::Encoded(bytes) => *bytes,
        _ => panic!("status response should be encoded"),
    };
    let status = match vox::schema_deser::schema_deserialize_response::<
        Result<StatusSnapshot, vox::VoxError>,
    >(ret_bytes, method_id, &with_tracker.tracker)
    .expect("status response should decode")
    {
        Ok(status) => status,
        Err(error) => panic!("status call returned error: {error:?}"),
    };
    assert_eq!(
        status,
        StatusSnapshot {
            active: vec![StatusRun {
                id: 7,
                state: StatusState::Running,
                target_pid: Some(4242),
                label: "current".to_string(),
            }],
            history: vec![StatusRun {
                id: 3,
                state: StatusState::Idle,
                target_pid: None,
                label: "previous".to_string(),
            }],
            daemon_started_unix_ns: 123_456,
        }
    );
}

// r[verify rpc.one-service-per-connection]
#[tokio::test]
async fn root_factory_routes_peer_requested_service_from_handshake() {
    use std::sync::{Arc, Mutex};

    let (client_link, server_link) = memory_link_pair(16);
    let seen_root_service = Arc::new(Mutex::new(None::<String>));

    let factory = vox::router_fn({
        let seen_root_service = Arc::clone(&seen_root_service);
        move |request: &vox::ConnectionRequest| -> Result<vox::ConnectionRoute, vox::Metadata> {
            if request.is_root() {
                *seen_root_service.lock().unwrap() = Some(request.service().to_string());
            }
            match request.service() {
                "StatusProbe" => Ok(vox::ConnectionRoute::handle(StatusProbeDispatcher::new(
                    StatusProbeService,
                ))),
                "Noop" => Ok(vox::ConnectionRoute::handle(())),
                _ => Err(Default::default()),
            }
        }
    });

    let server = tokio::spawn(async move {
        vox::acceptor_on(server_link)
            .on_connection(factory)
            .establish::<vox::NoopClient>()
            .await
            .expect("server establish")
    });

    let client = vox::initiator_on(client_link)
        .establish::<StatusProbeClient>()
        .await
        .expect("client establish");
    let _server_guard = server.await.expect("server task");

    assert_eq!(
        seen_root_service.lock().unwrap().as_deref(),
        Some("StatusProbe")
    );
    let status = client.status().await.expect("root status call");
    assert_eq!(status.daemon_started_unix_ns, 123_456);
}

// r[verify rpc.one-service-per-connection]
#[tokio::test]
async fn root_service_mismatch_is_rejected_before_calls() {
    let (client_link, server_link) = memory_link_pair(16);

    let factory = vox::router_fn(
        |_request: &vox::ConnectionRequest| -> Result<vox::ConnectionRoute, vox::Metadata> {
            Ok(vox::ConnectionRoute::handle(()))
        },
    );

    let server = tokio::spawn(async move {
        vox::acceptor_on(server_link)
            .on_connection(factory)
            .establish::<vox::NoopClient>()
            .await
    });
    let client = tokio::spawn(async move {
        vox::initiator_on(client_link)
            .establish::<StatusProbeClient>()
            .await
    });

    let (server_result, client_result) = tokio::join!(server, client);
    let error = match server_result.expect("server task") {
        Ok(_) => panic!("StatusProbe root should not establish against a Noop handler"),
        Err(error) => error,
    };
    let _ = client_result.expect("client task");

    let vox::SessionError::Rejected(metadata) = error else {
        panic!("expected rejection, got {error:?}");
    };
    assert_eq!(
        vox::metadata_get_str(&metadata, "vox-service-requested"),
        Some("StatusProbe")
    );
    assert_eq!(
        vox::metadata_get_str(&metadata, "vox-service-available"),
        Some("Noop")
    );
    assert_eq!(
        vox::metadata_get_str(&metadata, "error"),
        Some("requested vox service \"StatusProbe\", but this endpoint serves \"Noop\"")
    );
}

// r[verify rpc.one-service-per-connection]
#[tokio::test]
async fn virtual_service_mismatch_is_rejected_before_calls() {
    let (client_link, server_link) = memory_link_pair(16);

    let factory = vox::router_fn(
        |_request: &vox::ConnectionRequest| -> Result<vox::ConnectionRoute, vox::Metadata> {
            Ok(vox::ConnectionRoute::handle(()))
        },
    );

    let server = tokio::spawn(async move {
        vox::acceptor_on(server_link)
            .on_connection(factory)
            .establish::<vox::NoopClient>()
            .await
            .expect("server establish")
    });

    let root = vox::initiator_on(client_link)
        .establish::<vox::NoopClient>()
        .await
        .expect("client establish");

    let _server_guard = server.await.expect("server task");
    let session = root.session.clone().unwrap();
    let error = match session
        .open::<StatusProbeClient>(ConnectionSettings {
            parity: Parity::Odd,
            max_concurrent_requests: 64,
            initial_channel_credit: 16,
        })
        .await
    {
        Ok(_) => panic!("StatusProbe vconn should not establish against a Noop endpoint"),
        Err(error) => error,
    };

    let vox::SessionError::Rejected(metadata) = error else {
        panic!("expected rejection, got {error:?}");
    };
    assert_eq!(
        vox::metadata_get_str(&metadata, "vox-service-requested"),
        Some("StatusProbe")
    );
    assert_eq!(
        vox::metadata_get_str(&metadata, "vox-service-available"),
        Some("Noop")
    );
    assert_eq!(
        vox::metadata_get_str(&metadata, "error"),
        Some("requested vox service \"StatusProbe\", but this endpoint serves \"Noop\"")
    );
}

#[tokio::test]
async fn service_factory_routes_virtual_connections() {
    let (client_link, server_link) = memory_link_pair(16);

    let factory = vox::router_fn(
        |request: &vox::ConnectionRequest| -> Result<vox::ConnectionRoute, vox::Metadata> {
            match request.service() {
                "Echo" => Ok(vox::ConnectionRoute::handle(EchoDispatcher::new(
                    EchoService,
                ))),
                "Adder" => Ok(vox::ConnectionRoute::handle(AdderDispatcher::new(
                    AdderService,
                ))),
                "Noop" => Ok(vox::ConnectionRoute::handle(())),
                _ => Err(Default::default()),
            }
        },
    );

    let server = tokio::spawn(async move {
        vox::acceptor_on(server_link)
            .on_connection(factory)
            .establish::<vox::NoopClient>()
            .await
            .expect("server establish")
    });

    let root = vox::initiator_on(client_link)
        .establish::<vox::NoopClient>()
        .await
        .expect("client establish");

    let _server_guard = server.await.expect("server task");
    let session = root.session.clone().unwrap();

    // Open a typed Echo vconn
    let echo: EchoClient = session
        .open(ConnectionSettings {
            parity: Parity::Odd,
            max_concurrent_requests: 64,
            initial_channel_credit: 16,
        })
        .await
        .expect("open Echo vconn");

    let result = echo.echo(42).await.expect("echo call");
    assert_eq!(result, 42);

    // Open a typed Adder vconn
    let adder: AdderClient = session
        .open(ConnectionSettings {
            parity: Parity::Odd,
            max_concurrent_requests: 64,
            initial_channel_credit: 16,
        })
        .await
        .expect("open Adder vconn");

    let result = adder.add(3, 4).await.expect("add call");
    assert_eq!(result, 7);
}

#[tokio::test]
async fn service_factory_rejects_unknown_service() {
    let (client_link, server_link) = memory_link_pair(16);

    let factory = vox::router_fn(
        |request: &vox::ConnectionRequest| -> Result<vox::ConnectionRoute, vox::Metadata> {
            match request.service() {
                "Echo" => Ok(vox::ConnectionRoute::handle(EchoDispatcher::new(
                    EchoService,
                ))),
                "Noop" => Ok(vox::ConnectionRoute::handle(())),
                _ => Err(Default::default()),
            }
        },
    );

    let server = tokio::spawn(async move {
        vox::acceptor_on(server_link)
            .on_connection(factory)
            .establish::<vox::NoopClient>()
            .await
            .expect("server establish")
    });

    let root = vox::initiator_on(client_link)
        .establish::<vox::NoopClient>()
        .await
        .expect("client establish");

    let _server_guard = server.await.expect("server task");
    let session = root.session.clone().unwrap();

    // Adder is not in the factory — should be rejected
    let result = session
        .open::<AdderClient>(ConnectionSettings {
            parity: Parity::Odd,
            max_concurrent_requests: 64,
            initial_channel_credit: 16,
        })
        .await;

    assert!(result.is_err(), "unknown service should be rejected");
}
