use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;

use spec_proto::{
    Canvas, Color, Config, GnarlyPayload, LookupError, MathError, Measurement, Message, Person,
    Point, Profile, Record, Rectangle, Shape, Status, Tag, TaggedPoint, Testbed, TestbedClient,
    TestbedDispatcher, Tree,
};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use vox::{Rx, Tx};
use vox_core::{
    DriverReplySink, SessionHandle, acceptor_conduit, acceptor_on, acceptor_transport,
    memory_link_pair,
};
use vox_stream::StreamLink;
use vox_types::{RequestCall, SelfRef};
use vox_websocket::WsLink;

const SUBJECT_WAIT_HEARTBEAT: Duration = Duration::from_millis(500);
/// Spawn a task that catches panics and makes them loud.
///
/// If the spawned future panics, the panic message is printed to stderr
/// immediately and then re-raised. This prevents the silent-task-panic
/// problem where tokio tasks panic and nobody notices, causing mysterious
/// timeouts in tests.
pub fn spawn_loud<F>(fut: F) -> moire::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    moire::task::spawn(async move {
        // Inner spawn so we can catch the panic via JoinError
        let inner = tokio::task::spawn(fut);
        match inner.await {
            Ok(v) => v,
            Err(e) if e.is_panic() => {
                let panic = e.into_panic();
                let msg = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| format!("{panic:?}"));
                eprintln!("\n\n!!! SPAWNED TASK PANICKED !!!\n{msg}\n");
                std::panic::resume_unwind(panic);
            }
            Err(e) => {
                panic!("spawned task failed: {e}");
            }
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubjectLanguage {
    Rust,
    Swift,
    TypeScript,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubjectTestTransport {
    Tcp,
    Ws,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubjectSpec {
    pub language: SubjectLanguage,
    pub transport: SubjectTestTransport,
}

impl SubjectSpec {
    pub const fn tcp(language: SubjectLanguage) -> Self {
        Self {
            language,
            transport: SubjectTestTransport::Tcp,
        }
    }

    pub const fn ws(language: SubjectLanguage) -> Self {
        Self {
            language,
            transport: SubjectTestTransport::Ws,
        }
    }
}

#[derive(Clone)]
struct NoopHandler;

impl vox_types::Handler<DriverReplySink> for NoopHandler {
    async fn handle(
        &self,
        _call: SelfRef<RequestCall<'static>>,
        _reply: DriverReplySink,
        _schemas: std::sync::Arc<vox_types::SchemaRecvTracker>,
    ) {
    }
}

pub fn workspace_root() -> &'static std::path::Path {
    // `spec/spec-tests` → `spec` → workspace root
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
}

pub fn subject_cmd() -> String {
    match std::env::var("SUBJECT_CMD") {
        Ok(s) if !s.trim().is_empty() => s,
        _ => subject_cmd_for_language(SubjectLanguage::Rust),
    }
}

pub fn subject_cmd_for_language(language: SubjectLanguage) -> String {
    match language {
        SubjectLanguage::Rust => {
            let exe = format!("subject-rust{}", std::env::consts::EXE_SUFFIX);
            let debug = workspace_root().join("target").join("debug").join(&exe);
            if debug.exists() {
                debug.display().to_string()
            } else {
                workspace_root()
                    .join("target")
                    .join("release")
                    .join(&exe)
                    .display()
                    .to_string()
            }
        }
        SubjectLanguage::Swift => swift_subject_binary()
            .unwrap_or_else(|err| panic!("failed to prepare Swift subject: {err}")),
        SubjectLanguage::TypeScript => "./typescript/subject/subject-ts.sh".to_string(),
    }
}

fn swift_subject_binary() -> Result<String, String> {
    static SWIFT_SUBJECT_BINARY: OnceLock<Result<String, String>> = OnceLock::new();

    SWIFT_SUBJECT_BINARY
        .get_or_init(|| {
            let subject_dir = workspace_root().join("swift").join("subject");
            let binary = subject_dir
                .join(".build")
                .join("release")
                .join(format!("subject-swift{}", std::env::consts::EXE_SUFFIX));

            eprintln!("[subject:swift] preparing release subject at {}", binary.display());
            let output = std::process::Command::new("swift")
                .arg("build")
                .arg("-c")
                .arg("release")
                .arg("--product")
                .arg("subject-swift")
                .current_dir(&subject_dir)
                .output()
                .map_err(|err| format!("failed to run swift build for subject-swift: {err}"))?;

            if !output.status.success() {
                return Err(format!(
                    "swift build -c release --product subject-swift failed with {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }

            if !binary.exists() {
                return Err(format!(
                    "swift build -c release --product subject-swift completed, but {} does not exist",
                    binary.display()
                ));
            }

            eprintln!("[subject:swift] release subject ready at {}", binary.display());
            Ok(binary.display().to_string())
        })
        .clone()
}

fn subject_transport() -> SubjectTestTransport {
    match std::env::var("SPEC_TRANSPORT")
        .ok()
        .unwrap_or_else(|| "tcp".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "ws" => SubjectTestTransport::Ws,
        _ => SubjectTestTransport::Tcp,
    }
}

pub fn run_async<T>(f: impl Future<Output = T>) -> T {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(f)
}

#[derive(Clone, Default)]
struct TestbedService;

impl TestbedService {
    fn new() -> Self {
        Self::default()
    }
}

async fn stream_values(count: u32, output: Tx<i32>) {
    for i in 0..count as i32 {
        if output.send(i).await.is_err() {
            break;
        }
    }
    output.close(Default::default()).await.ok();
}

impl Testbed for TestbedService {
    async fn echo(&self, message: String) -> String {
        message
    }

    async fn reverse(&self, message: String) -> String {
        message.chars().rev().collect()
    }

    async fn divide(&self, dividend: i64, divisor: i64) -> Result<i64, MathError> {
        if divisor == 0 {
            Err(MathError::DivisionByZero)
        } else {
            dividend.checked_div(divisor).ok_or(MathError::Overflow)
        }
    }

    async fn lookup(&self, id: u32) -> Result<Person, LookupError> {
        match id {
            1 => Ok(Person {
                name: "Alice".to_string(),
                age: 30,
                email: Some("alice@example.com".to_string()),
            }),
            2 => Ok(Person {
                name: "Bob".to_string(),
                age: 25,
                email: None,
            }),
            3 => Ok(Person {
                name: "Charlie".to_string(),
                age: 35,
                email: Some("charlie@example.com".to_string()),
            }),
            100..=199 => Err(LookupError::AccessDenied),
            _ => Err(LookupError::NotFound),
        }
    }

    async fn sum(&self, mut numbers: Rx<i32>) -> i64 {
        let mut total: i64 = 0;
        while let Ok(Some(n)) = numbers.recv().await {
            let n = n.get();
            total += *n as i64;
        }
        total
    }

    async fn generate(&self, count: u32, output: Tx<i32>) {
        stream_values(count, output).await;
    }

    async fn transform(&self, mut input: Rx<String>, output: Tx<String>) {
        while let Ok(Some(s)) = input.recv().await {
            let s = s.get();
            let _ = output.send(s.clone()).await;
        }
        output.close(Default::default()).await.ok();
    }

    async fn post_reply_generate(&self, output: Tx<i32>) {
        spawn_loud(async move {
            moire::time::sleep(Duration::from_millis(10)).await;
            for i in 0..5 {
                if output.send(i).await.is_err() {
                    break;
                }
            }
            output.close(Default::default()).await.ok();
        });
    }

    async fn post_reply_sum(&self, mut input: Rx<i32>, result: Tx<i64>) {
        spawn_loud(async move {
            let mut total: i64 = 0;
            while let Ok(Some(n)) = input.recv().await {
                let n = n.get();
                total += *n as i64;
            }
            let _ = result.send(total).await;
            result.close(Default::default()).await.ok();
        });
    }

    async fn echo_point(&self, point: Point) -> Point {
        point
    }

    async fn create_person(&self, name: String, age: u8, email: Option<String>) -> Person {
        Person { name, age, email }
    }

    async fn rectangle_area(&self, rect: Rectangle) -> f64 {
        let width = (rect.bottom_right.x - rect.top_left.x).abs() as f64;
        let height = (rect.bottom_right.y - rect.top_left.y).abs() as f64;
        width * height
    }

    async fn parse_color(&self, name: String) -> Option<Color> {
        match name.to_lowercase().as_str() {
            "red" => Some(Color::Red),
            "green" => Some(Color::Green),
            "blue" => Some(Color::Blue),
            _ => None,
        }
    }

    async fn shape_area(&self, shape: Shape) -> f64 {
        match shape {
            Shape::Circle { radius } => std::f64::consts::PI * radius * radius,
            Shape::Rectangle { width, height } => width * height,
            Shape::Point => 0.0,
        }
    }

    async fn create_canvas(&self, name: String, shapes: Vec<Shape>, background: Color) -> Canvas {
        Canvas {
            name,
            shapes,
            background,
        }
    }

    async fn process_message(&self, msg: Message) -> Message {
        match msg {
            Message::Text(s) => Message::Text(format!("processed: {s}")),
            Message::Number(n) => Message::Number(n * 2),
            Message::Data(d) => Message::Data(d.into_iter().rev().collect()),
        }
    }

    async fn get_points(&self, count: u32) -> Vec<Point> {
        (0..count as i32)
            .map(|i| Point { x: i, y: i * 2 })
            .collect()
    }

    async fn swap_pair(&self, pair: (i32, String)) -> (String, i32) {
        (pair.1, pair.0)
    }

    async fn echo_bytes(&self, data: Vec<u8>) -> Vec<u8> {
        data
    }

    async fn echo_bool(&self, b: bool) -> bool {
        b
    }

    async fn echo_u64(&self, n: u64) -> u64 {
        n
    }

    async fn echo_option_string(&self, s: Option<String>) -> Option<String> {
        s
    }

    async fn sum_large(&self, mut numbers: Rx<i32>) -> i64 {
        let mut total: i64 = 0;
        while let Ok(Some(n)) = numbers.recv().await {
            let n = n.get();
            total += *n as i64;
        }
        total
    }

    async fn generate_large(&self, count: u32, output: Tx<i32>) {
        stream_values(count, output).await;
    }

    async fn all_colors(&self) -> Vec<Color> {
        vec![Color::Red, Color::Green, Color::Blue]
    }

    async fn describe_point(&self, label: String, x: i32, y: i32, active: bool) -> TaggedPoint {
        TaggedPoint {
            label,
            x,
            y,
            active,
        }
    }

    async fn echo_shape(&self, shape: Shape) -> Shape {
        shape
    }

    async fn echo_status_v1(&self, status: Status) -> Status {
        status
    }

    async fn echo_tag_v1(&self, tag: Tag) -> Tag {
        tag
    }

    async fn echo_profile(&self, profile: Profile) -> Profile {
        profile
    }

    async fn echo_record(&self, record: Record) -> Record {
        record
    }

    async fn echo_status(&self, status: Status) -> Status {
        status
    }

    async fn echo_tag(&self, tag: Tag) -> Tag {
        tag
    }

    async fn echo_measurement(&self, m: Measurement) -> Measurement {
        m
    }

    async fn echo_config(&self, c: Config) -> Config {
        c
    }

    async fn echo_gnarly(&self, payload: GnarlyPayload) -> GnarlyPayload {
        payload
    }

    async fn echo_tree(&self, tree: Tree) -> Tree {
        tree
    }
}

/// Spawn the subject binary, telling it to connect to `peer_addr`.
pub async fn spawn_subject(peer_addr: &str) -> Result<Child, String> {
    spawn_subject_cmd_with_env(&subject_cmd(), peer_addr, &[]).await
}

fn spawn_subject_log_pump<R>(reader: R, pid: u32, stream: &'static str)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => eprintln!("[subject:{pid}:{stream}] {line}"),
                Ok(None) => break,
                Err(err) => {
                    eprintln!("[subject:{pid}:{stream}] log read error: {err}");
                    break;
                }
            }
        }
    });
}

async fn wait_for_child_exit(child: &mut Child, reason: &str, timeout: Duration) -> bool {
    let pid = child.id().unwrap_or_default();
    match child.try_wait() {
        Ok(Some(status)) => {
            eprintln!("[subject:{pid}] exited during {reason}: {status}");
            return true;
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("[subject:{pid}] try_wait failed during {reason}: {err}");
        }
    }

    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => {
            eprintln!("[subject:{pid}] exited during {reason}: {status}");
            true
        }
        Ok(Err(err)) => {
            eprintln!("[subject:{pid}] wait failed during {reason}: {err}");
            false
        }
        Err(_) => false,
    }
}

async fn terminate_child(child: &mut Child, reason: &str) {
    let pid = child.id().unwrap_or_default();
    if wait_for_child_exit(child, reason, Duration::from_millis(0)).await {
        return;
    }

    eprintln!("[subject:{pid}] terminating: {reason}");
    if let Err(err) = child.start_kill() {
        eprintln!("[subject:{pid}] start_kill failed during {reason}: {err}");
        return;
    }

    match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
        Ok(Ok(status)) => {
            eprintln!("[subject:{pid}] reaped after termination: {status}");
        }
        Ok(Err(err)) => {
            eprintln!("[subject:{pid}] wait after termination failed: {err}");
        }
        Err(_) => {
            eprintln!("[subject:{pid}] timed out waiting to reap after termination");
        }
    }
}

async fn spawn_subject_cmd_with_env(
    cmd: &str,
    peer_addr: &str,
    extra_env: &[(&str, &str)],
) -> Result<Child, String> {
    let extra_env_desc = if extra_env.is_empty() {
        "<none>".to_string()
    } else {
        extra_env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    eprintln!("[subject:spawn] cmd={cmd:?} peer_addr={peer_addr:?} extra_env={extra_env_desc}");

    let mut command = if cmd.ends_with(".sh") {
        let mut c = Command::new("sh");
        c.arg("-lc").arg(cmd);
        c
    } else {
        Command::new(cmd)
    };
    command
        .current_dir(workspace_root())
        .env("PEER_ADDR", peer_addr)
        .env("VOX_DLOG", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.kill_on_drop(true);
    for (k, v) in extra_env {
        command.env(k, v);
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to spawn subject: {e}"))?;
    let pid = child.id().unwrap_or_default();
    eprintln!("[subject:{pid}] spawned");

    if let Some(stdout) = child.stdout.take() {
        spawn_subject_log_pump(stdout, pid, "stdout");
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_subject_log_pump(stderr, pid, "stderr");
    }

    // If it crashes immediately (non-zero exit), surface that early.
    // A fast successful exit (code 0) is fine - the test just completed quickly.
    tokio::time::sleep(Duration::from_millis(10)).await;
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())?
        && !status.success()
    {
        eprintln!("[subject:{pid}] crashed immediately: {status}");
        return Err(format!("subject crashed immediately with {status}"));
    }

    Ok(child)
}

/// Listen on a random TCP port, upgrade incoming connection to WebSocket,
/// complete the vox handshake, and return a ready `TestbedClient`.
pub async fn accept_subject_ws(cmd: &str) -> Result<(TestbedClient, Child, SessionHandle), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    let ws_url = format!("ws://127.0.0.1:{port}/");

    let child = spawn_subject_cmd_with_env(cmd, &ws_url, &[]).await?;

    // Use a timeout to catch subjects that fail to connect.
    let mut child = child;
    let (tcp_stream, _) =
        match tokio::time::timeout(Duration::from_secs(5), listener.accept()).await {
            Ok(Ok(accepted)) => accepted,
            Ok(Err(err)) => {
                terminate_child(&mut child, "WebSocket accept failed").await;
                return Err(format!("accept: {err}"));
            }
            Err(_) => {
                terminate_child(
                    &mut child,
                    "timed out waiting for WebSocket subject to connect",
                )
                .await;
                return Err("timed out waiting for WebSocket subject to connect".to_string());
            }
        };
    tcp_stream.set_nodelay(true).ok();

    let ws = match WsLink::server(tcp_stream).await {
        Ok(ws) => ws,
        Err(err) => {
            terminate_child(&mut child, "WebSocket upgrade failed").await;
            return Err(format!("WebSocket upgrade: {err}"));
        }
    };

    let client = match acceptor_on(ws)
        .on_connection(TestbedDispatcher::new(TestbedService::new()))
        .establish::<TestbedClient>()
        .await
    {
        Ok(client) => client,
        Err(err) => {
            terminate_child(&mut child, "WebSocket handshake failed").await;
            return Err(format!("handshake: {err}"));
        }
    };
    let sh = client.session.clone().unwrap();

    Ok((client, child, sh))
}

pub async fn accept_subject() -> Result<(TestbedClient, Child, SessionHandle), String> {
    let spec = SubjectSpec {
        language: SubjectLanguage::Rust,
        transport: subject_transport(),
    };
    accept_subject_spec(spec).await
}

pub async fn accept_subject_spec(
    spec: SubjectSpec,
) -> Result<(TestbedClient, Child, SessionHandle), String> {
    let cmd = subject_cmd_for_language(spec.language);
    match spec.transport {
        SubjectTestTransport::Tcp => accept_subject_tcp(&cmd).await,
        SubjectTestTransport::Ws => accept_subject_ws(&cmd).await,
    }
}

/// Accept a subject over TCP given a custom command string.
pub async fn accept_subject_cmd_tcp(
    cmd: &str,
) -> Result<(TestbedClient, Child, SessionHandle), String> {
    accept_subject_tcp(cmd).await
}

/// Spawn a subject, establish a connection, run a test closure, and clean up.
///
/// Monitors the child process in a background task — if the subject dies,
/// the session handle is dropped so pending calls fail immediately instead
/// of hanging until a timeout.
pub async fn with_subject<F, T>(spec: SubjectSpec, f: F) -> Result<T, String>
where
    F: AsyncFnOnce(&TestbedClient) -> Result<T, String>,
{
    let cmd = subject_cmd_for_language(spec.language);
    with_subject_cmd(spec, &cmd, f).await
}

/// Like [`with_subject`] but with a custom command string (e.g. for evolved TS subjects).
pub async fn with_subject_cmd<F, T>(spec: SubjectSpec, cmd: &str, f: F) -> Result<T, String>
where
    F: AsyncFnOnce(&TestbedClient) -> Result<T, String>,
{
    let (client, mut child, session_handle) = match spec.transport {
        SubjectTestTransport::Tcp => accept_subject_tcp(cmd).await?,
        SubjectTestTransport::Ws => accept_subject_ws(cmd).await?,
    };

    let child_pid = child.id().unwrap_or_default();
    let mut child_waited = false;
    let result = {
        let child_wait = child.wait();
        tokio::pin!(child_wait);
        tokio::select! {
            result = f(&client) => result,
            status = &mut child_wait => {
                child_waited = true;
                let msg = match status {
                    Ok(status) => format!("subject (pid={child_pid}) exited: {status}"),
                    Err(err) => format!("subject (pid={child_pid}) wait error: {err}"),
                };
                eprintln!("[harness] {msg}");
                Err(format!("subject died during test: {msg}"))
            }
        }
    };

    drop(client);
    drop(session_handle);
    if !child_waited
        && !wait_for_child_exit(&mut child, "session close", Duration::from_millis(500)).await
    {
        terminate_child(&mut child, "test completed before subject exited").await;
    }

    result
}

pub async fn accept_subject_with_transport(
    transport: SubjectTestTransport,
) -> Result<(TestbedClient, Child, SessionHandle), String> {
    accept_subject_spec(SubjectSpec {
        language: SubjectLanguage::Rust,
        transport,
    })
    .await
}

/// Spawn a subject in `server-listen` mode, wait for it to announce its
/// bound address on stdout (`LISTEN_ADDR=127.0.0.1:PORT`), then return
/// the address string and the child process handle.
///
/// Spawns the process directly (without the normal log pump) so we can
/// read the `LISTEN_ADDR=` line from stdout before handing it off.
/// After reading the address, stderr is pumped to the test output as usual.
pub async fn spawn_server_subject(spec: SubjectSpec) -> Result<(String, Child), String> {
    if spec.transport != SubjectTestTransport::Tcp {
        return Err("server-listen mode is only supported for TCP transport".to_string());
    }

    let cmd = subject_cmd_for_language(spec.language);
    eprintln!(
        "[subject:spawn] cmd={cmd:?} peer_addr=<server-listen> extra_env=SUBJECT_MODE=server-listen LISTEN_PORT=0"
    );

    let mut command = if cmd.ends_with(".sh") {
        let mut c = Command::new("sh");
        c.arg("-lc").arg(cmd);
        c
    } else {
        Command::new(cmd)
    };
    command
        .current_dir(workspace_root())
        .env("PEER_ADDR", "unused")
        .env("SUBJECT_MODE", "server-listen")
        .env("LISTEN_PORT", "0")
        .env("VOX_DLOG", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped()) // we read this ourselves
        .stderr(Stdio::piped()); // pumped after addr is read
    command.kill_on_drop(true);

    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to spawn server subject: {e}"))?;
    let pid = child.id().unwrap_or_default();
    eprintln!("[subject:{pid}] spawned (server-listen)");

    // Read stdout until we see LISTEN_ADDR=.  We must do this before
    // handing stdout to the log pump, because the pump would consume it.
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_child(&mut child, "server subject had no stdout").await;
            return Err("no stdout from server subject".to_string());
        }
    };
    let addr = match tokio::time::timeout(Duration::from_secs(10), async {
        use tokio::io::AsyncBufReadExt;
        let mut reader = tokio::io::BufReader::new(&mut stdout);
        let mut line = String::new();
        loop {
            line.clear();
            reader
                .read_line(&mut line)
                .await
                .map_err(|e| format!("reading server subject stdout: {e}"))?;
            let trimmed = line.trim();
            if let Some(addr) = trimmed.strip_prefix("LISTEN_ADDR=") {
                return Ok::<String, String>(addr.to_string());
            }
            if line.is_empty() {
                return Err("server subject closed stdout without announcing address".to_string());
            }
            // Forward any other stdout lines as log output.
            eprintln!("[subject:{pid}:stdout] {trimmed}");
        }
    })
    .await
    {
        Ok(Ok(addr)) => addr,
        Ok(Err(err)) => {
            terminate_child(
                &mut child,
                "server subject failed before announcing address",
            )
            .await;
            return Err(err);
        }
        Err(_) => {
            terminate_child(
                &mut child,
                "timed out waiting for server subject to announce listen address",
            )
            .await;
            return Err(
                "timed out waiting for server subject to announce listen address".to_string(),
            );
        }
    };

    // Hand the rest of stdout and all of stderr to the log pump.
    spawn_subject_log_pump(stdout, pid, "stdout");
    if let Some(stderr) = child.stderr.take() {
        spawn_subject_log_pump(stderr, pid, "stderr");
    }

    eprintln!("[subject:{pid}] server-listen ready at {addr}");
    Ok((addr, child))
}

/// Run a cross-language scenario: spawn `server_spec` in server-listen mode,
/// then spawn `client_spec` as a client pointing at the server.
/// The harness orchestrates but is not in the data path — all traffic flows
/// directly between the two subjects.
pub fn run_cross_language_scenario(
    server_spec: SubjectSpec,
    client_spec: SubjectSpec,
    scenario: &str,
) {
    let scenario = scenario.to_string();
    let result: Result<(), String> = run_async(async move {
        if server_spec.transport != SubjectTestTransport::Tcp
            || client_spec.transport != SubjectTestTransport::Tcp
        {
            // Only TCP cross-language supported for now.
            return Ok(());
        }

        let (server_addr, mut server_child) = spawn_server_subject(server_spec).await?;

        let client_cmd = subject_cmd_for_language(client_spec.language);
        let mut client_child = match spawn_subject_cmd_with_env(
            &client_cmd,
            &server_addr,
            &[("SUBJECT_MODE", "client"), ("CLIENT_SCENARIO", &scenario)],
        )
        .await
        {
            Ok(child) => child,
            Err(err) => {
                terminate_child(&mut server_child, "client subject failed to spawn").await;
                return Err(err);
            }
        };

        let status = match tokio::time::timeout(Duration::from_secs(15), client_child.wait()).await
        {
            Ok(Ok(status)) => status,
            Ok(Err(err)) => {
                terminate_child(&mut server_child, "client subject wait failed").await;
                return Err(format!("wait on client subject: {err}"));
            }
            Err(_) => {
                terminate_child(&mut client_child, "cross-language client timed out").await;
                terminate_child(&mut server_child, "cross-language scenario timed out").await;
                return Err(format!("cross-language scenario `{scenario}` timed out"));
            }
        };

        if !wait_for_child_exit(
            &mut server_child,
            "cross-language client exit",
            Duration::from_millis(500),
        )
        .await
        {
            terminate_child(&mut server_child, "cross-language scenario completed").await;
        }

        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "cross-language scenario `{scenario}` failed with status {status}"
            ))
        }
    });
    result.unwrap();
}

pub fn run_subject_client_scenario(spec: SubjectSpec, scenario: &str) {
    let scenario = scenario.to_string();
    let result: Result<(), String> = run_async(async move {
        match spec.transport {
            SubjectTestTransport::Tcp => {
                run_subject_client_scenario_tcp(spec.language, &scenario).await
            }
            SubjectTestTransport::Ws => {
                run_subject_client_scenario_ws(spec.language, &scenario).await
            }
        }
    });
    result.unwrap();
}

async fn run_subject_client_scenario_tcp(
    language: SubjectLanguage,
    scenario: &str,
) -> Result<(), String> {
    let cmd = subject_cmd_for_language(language);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?;

    let mut child = spawn_subject_cmd_with_env(
        &cmd,
        &addr.to_string(),
        &[("SUBJECT_MODE", "client"), ("CLIENT_SCENARIO", scenario)],
    )
    .await?;

    let accept_task = tokio::spawn(async move {
        let (stream, _) = match listener.accept().await {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[harness] client-scenario accept error: {e}");
                return;
            }
        };
        stream.set_nodelay(true).ok();
        match acceptor_on(StreamLink::tcp(stream))
            .on_connection(TestbedDispatcher::new(TestbedService::new()))
            .establish::<TestbedClient>()
            .await
        {
            Ok(_client) => {
                std::future::pending::<()>().await;
            }
            Err(e) => {
                eprintln!("[harness] client-scenario handshake error: {e}");
            }
        }
    });

    let status = match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => {
            accept_task.abort();
            terminate_child(&mut child, "subject client wait failed").await;
            return Err(format!("wait on subject process: {err}"));
        }
        Err(_) => {
            accept_task.abort();
            terminate_child(&mut child, "subject client scenario timed out").await;
            return Err(format!("subject client scenario `{scenario}` timed out"));
        }
    };

    accept_task.abort();
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "subject client scenario `{scenario}` failed with status {status}"
        ))
    }
}

async fn run_subject_client_scenario_ws(
    language: SubjectLanguage,
    scenario: &str,
) -> Result<(), String> {
    let cmd = subject_cmd_for_language(language);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    let ws_url = format!("ws://127.0.0.1:{port}/");

    let mut child = spawn_subject_cmd_with_env(
        &cmd,
        &ws_url,
        &[("SUBJECT_MODE", "client"), ("CLIENT_SCENARIO", scenario)],
    )
    .await?;

    let accept_task = tokio::spawn(async move {
        let (tcp_stream, _) = match listener.accept().await {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[harness] ws client-scenario accept error: {e}");
                return;
            }
        };
        tcp_stream.set_nodelay(true).ok();
        let ws = match WsLink::server(tcp_stream).await {
            Ok(ws) => ws,
            Err(e) => {
                eprintln!("[harness] ws upgrade error: {e}");
                return;
            }
        };
        match acceptor_on(ws)
            .on_connection(TestbedDispatcher::new(TestbedService::new()))
            .establish::<TestbedClient>()
            .await
        {
            Ok(_client) => {
                std::future::pending::<()>().await;
            }
            Err(e) => {
                eprintln!("[harness] ws client-scenario handshake error: {e}");
            }
        }
    });

    let status = match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => {
            accept_task.abort();
            terminate_child(&mut child, "WebSocket subject client wait failed").await;
            return Err(format!("wait on subject process: {err}"));
        }
        Err(_) => {
            accept_task.abort();
            terminate_child(&mut child, "WebSocket subject client scenario timed out").await;
            return Err(format!(
                "subject client scenario (ws) `{scenario}` timed out"
            ));
        }
    };

    accept_task.abort();
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "subject client scenario (ws) `{scenario}` failed with status {status}"
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RustTransport {
    Mem,
    Tcp,
}

pub async fn accept_rust_inproc(transport: RustTransport) -> Result<TestbedClient, String> {
    match transport {
        RustTransport::Mem => {
            let (a, b) = memory_link_pair(64 * 1024);
            accept_rust_inproc_with_conduits(a, b).await
        }
        RustTransport::Tcp => {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| format!("bind: {e}"))?;
            let addr = listener
                .local_addr()
                .map_err(|e| format!("local_addr: {e}"))?;
            let connect_task =
                tokio::spawn(async move { tokio::net::TcpStream::connect(addr).await });
            let (server_stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("accept: {e}"))?;
            let client_stream = connect_task
                .await
                .map_err(|e| format!("connect task join: {e}"))?
                .map_err(|e| format!("connect: {e}"))?;
            server_stream.set_nodelay(true).unwrap();
            client_stream.set_nodelay(true).unwrap();
            accept_rust_inproc_with_conduits(
                StreamLink::tcp(client_stream),
                StreamLink::tcp(server_stream),
            )
            .await
        }
    }
}

async fn accept_rust_inproc_with_conduits<L>(
    client_link: L,
    server_link: L,
) -> Result<TestbedClient, String>
where
    L: vox_types::Link + Send + 'static,
    L::Tx: Send + 'static,
    L::Rx: Send + 'static,
    <L::Rx as vox_types::LinkRx>::Error: std::error::Error + Send + Sync + 'static,
{
    let (server_ready_tx, server_ready_rx) = oneshot::channel::<Result<(), String>>();
    let _server_task = tokio::spawn(async move {
        let (tx, mut rx) = vox_types::Link::split(server_link);
        let handshake_result = vox_core::handshake_as_acceptor(
            &tx,
            &mut rx,
            vox_types::ConnectionSettings {
                parity: vox_types::Parity::Even,
                max_concurrent_requests: 64,
                initial_channel_credit: 16,
            },
            vox_types::metadata().str("vox-service", "Noop").build(),
        )
        .await
        .map_err(|e| format!("server CBOR handshake: {e}"));
        let handshake_result = match handshake_result {
            Ok(r) => r,
            Err(err) => {
                let _ = server_ready_tx.send(Err(err));
                return;
            }
        };
        let server_conduit =
            vox_core::BareConduit::<vox_types::MessageFamily, _>::new(vox_types::SplitLink {
                tx,
                rx,
            });
        let setup = acceptor_conduit(server_conduit, handshake_result)
            .on_connection(TestbedDispatcher::new(TestbedService::new()))
            .establish::<TestbedClient>()
            .await
            .map_err(|e| format!("server handshake: {e}"));
        let server_caller_guard = match setup {
            Ok(parts) => parts,
            Err(err) => {
                let _ = server_ready_tx.send(Err(err));
                return;
            }
        };

        let _ = server_ready_tx.send(Ok(()));
        let _server_caller_guard = server_caller_guard;
        std::future::pending::<()>().await;
    });

    let (client_tx, mut client_rx) = vox_types::Link::split(client_link);
    let client_handshake = vox_core::handshake_as_initiator(
        &client_tx,
        &mut client_rx,
        vox_types::ConnectionSettings {
            parity: vox_types::Parity::Odd,
            max_concurrent_requests: 64,
            initial_channel_credit: 16,
        },
        vox_types::metadata().str("vox-service", "Noop").build(),
    )
    .await
    .map_err(|e| format!("client CBOR handshake: {e}"))?;
    let client_conduit =
        vox_core::BareConduit::<vox_types::MessageFamily, _>::new(vox_types::SplitLink {
            tx: client_tx,
            rx: client_rx,
        });
    let client = vox_core::initiator_conduit(client_conduit, client_handshake)
        .on_connection(NoopHandler)
        .establish::<TestbedClient>()
        .await
        .map_err(|e| format!("client handshake: {e}"))?;

    server_ready_rx
        .await
        .map_err(|e| format!("server task join: {e}"))??;

    Ok(client)
}

async fn accept_subject_tcp(cmd: &str) -> Result<(TestbedClient, Child, SessionHandle), String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?;

    let mut child = spawn_subject_cmd_with_env(cmd, &addr.to_string(), &[]).await?;
    let pid = child.id().unwrap_or_default();
    let wait_started = tokio::time::Instant::now();
    let wait_deadline = wait_started + Duration::from_secs(5);
    let mut heartbeat = tokio::time::interval(SUBJECT_WAIT_HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;

    let (stream, _) = loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok(accepted) => break accepted,
                    Err(err) => {
                        terminate_child(&mut child, "TCP accept failed").await;
                        return Err(format!("accept: {err}"));
                    }
                }
            }
            status = child.wait() => {
                let status = status.map_err(|e| format!("wait on subject process: {e}"))?;
                return Err(format!("subject exited before connecting: {status}"));
            }
            _ = tokio::time::sleep_until(wait_deadline) => {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|e| format!("try_wait on subject process: {e}"))?
                {
                    return Err(format!("subject exited before connecting: {status}"));
                }
                terminate_child(&mut child, "subject did not connect within 5s").await;
                return Err(format!(
                    "subject did not connect within 5s (pid={pid}, addr={addr}, elapsed={:?})",
                    wait_started.elapsed()
                ));
            }
            _ = heartbeat.tick() => {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|e| format!("try_wait on subject process: {e}"))?
                {
                    return Err(format!("subject exited while waiting for tcp connect: {status}"));
                }
                eprintln!(
                    "[subject:{pid}] waiting for tcp connect to {addr} (elapsed={:?})",
                    wait_started.elapsed()
                );
            }
        }
    };
    stream.set_nodelay(true).unwrap();

    let client = match acceptor_transport(StreamLink::tcp(stream))
        .on_connection(NoopHandler)
        .establish::<TestbedClient>()
        .await
    {
        Ok(client) => client,
        Err(err) => {
            terminate_child(&mut child, "TCP handshake failed").await;
            return Err(format!("handshake: {err}"));
        }
    };
    let sh = client.session.clone().unwrap();

    Ok((client, child, sh))
}
