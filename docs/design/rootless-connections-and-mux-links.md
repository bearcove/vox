# Rootless Connections and Mux Links

Status: tentative design note, not spec.

This note captures the current direction for simplifying Vox connection
lifecycle, removing the root-service footgun, and preserving the unusually
flexible proxy/topology cases that motivated virtual connections in the first
place.

If this direction survives review against real Vox users, the normative pieces
should move into `docs/content/spec/conn.md` and `docs/content/spec/rpc.md`
with Tracey requirements. Until then this file should not contain Tracey
requirement annotations.

## Problem

The current model has three concepts tangled together:

- a physical `Link`, which is already a dumb bidirectional message transport;
- a `Session`, which performs the Vox handshake and owns many connection IDs;
- `Connection`s, including a special service-bearing root connection with ID 0.

That root connection is the recurring footgun.

On the server side, accepting a link usually creates a root caller that is not
really an application client. Rust calls that value `NoopClient`. It is a
liveness token: drop it, and the root connection can close. That makes examples
and harnesses fragile. A server can accidentally stop listening to an accepted
peer by dropping something that looks like a useless no-op client.

On the client side, the same model leaks into generated clients and session
handles. The common HTTP/gRPC-shaped intuition is "connect, get a client, call
methods". Vox can support stranger topologies, but ordinary users should not
pay for them in the core API. The current API exposes the extra power through a
special root plus virtual connections, which makes the simple case look
stranger than it needs to and makes the advanced case depend on a fake root
service.

The important counterweight is not "Dodeca needs virtual connections exactly as
they exist today". It is weaker and cleaner: if some topology needs reverse
work, NAT traversal, proxying, or rendezvous, that topology should establish or
obtain another Vox link. Vox RPC itself should still see an ordinary link and
run an ordinary connection over it.

## Direction

Remove service-bearing root connections.

A fresh Vox RPC instance over a `Link` should establish exactly the service
surface that the opener asked for, plus protocol control needed to run that
surface. There should be no application-visible root service, no root caller,
and no `NoopClient` liveness token.

Keep `Link` dumb.

A `Link` remains a bidirectional frame source/sink: send one payload, receive
one payload, preserve order, apply backpressure at send, close gracefully. A
`Link` should not know about service schemas, operations, request IDs, channel
IDs, discovery, observability, or auth policy.

Keep the core one-link, one-connection, one-service.

The common path should not include mux concepts at all:

```text
tcp/ws/ffi/xpc/etc link
    -> one Vox RPC connection
    -> one requested service surface
```

Put topology tricks outside the core RPC connection.

Instead of making one Vox session contain many virtual connections, any
advanced topology should produce more links:

- a second TCP/Unix/XPC/FFI connection;
- a platform rendezvous that hands both sides a connected local link;
- a NAT-punching protocol that yields a link through the punched hole;
- an optional multiplexing transport that opens child links over one carrier.

Each produced link is just another `Link`. Vox RPC runs independently over
that link, including its own transport prologue, handshake, schema exchange,
request state, channels, and shutdown.

That gives us a smaller stack:

```text
ordinary transport:

    link -> Vox connection -> requested service

advanced transport or rendezvous, outside core RPC:

    mechanism -> link A -> Vox connection -> service A
              -> link B -> Vox connection -> service B
              -> link C -> Vox connection -> service C
```

The current names are probably wrong. Tentative vocabulary:

| Old | Tentative | Meaning |
| --- | --- | --- |
| link | link | Dumb bidirectional frame transport. |
| session | connection | A Vox RPC instance over one link. Stronger than a link because it has Vox handshake/schema/request state. |
| connection | optional extra link | Current virtual connection functionality, probably replaced by separate links produced by transports, rendezvous, or optional mux. |

The most important naming decision is not the exact word. It is that the
special root service disappears.

## Directionality

Physical link direction and logical service direction may be decoupled by
creating another link.

If Alice dials Bob, that only says Alice created the physical link. It should
not imply that only Alice can initiate all logical work forever. It also should
not imply the first Vox connection needs to become bidirectional service soup.

Current thesis:

- a Vox connection has one logical service opener;
- requests on that Vox connection are initiated by the opener;
- responses, request-scoped channels, cancellation, credit, and errors flow
  both ways as part of that service interaction;
- if the accepted side wants to call a service on the opener, it obtains
  another link and opens another Vox connection in that logical direction.

The "another link" mechanism can be boring: dial a known address, connect over
FFI, ask an HTTP cell to establish a companion connection, use XPC rendezvous,
or run a NAT-punching protocol that returns a connected link. Vox does not need
to know which one happened.

Likewise, if a system needs the peer that physically established the connection
to be the peer serving the Vox service, that does not need to be a Vox RPC
primitive. A lower-level wrapper protocol can negotiate, authenticate, punch
holes, exchange handles, or otherwise set up the actual link. Once the wrapper
hands Vox a `Link`, Vox only cares which side opens the Vox connection and
which service is being requested.

This avoids the confusing "one connection where both peers are arbitrary
clients and servers at once" model without forcing every user to carry
NAT/proxy/topology machinery.

Open question: callbacks. If a method hands out a channel, the stream already
has bidirectional protocol traffic. If a method wants the peer to invoke an
entire service, that should probably be represented as an explicit second link
or service capability, not as accidental bidirectionality on the same service
connection.

## Optional Mux Links

If Vox grows a mux primitive, it should be transport-shaped, not RPC-shaped,
and it should be optional. The core redesign does not depend on it.

Sketch:

```rust
trait MuxCarrier {
    type Link: vox::Link;
    type Evidence;

    async fn open_link(&self, metadata: Metadata) -> Result<AcceptedLink<Self::Link>, Error>;
    async fn accept_link(&self) -> Result<Option<AcceptedLink<Self::Link>>, Error>;
}

struct AcceptedLink<L> {
    link: L,
    evidence: PeerEvidence,
}
```

Names are placeholders. The shape matters:

- opening a child link is below Vox RPC;
- accepting a child link yields a fresh `Link`;
- any peer evidence travels beside the link, not inside arbitrary user
  metadata;
- Vox then performs normal prologue/handshake/schema negotiation on the child
  link.

`open_link` metadata is transport/mux metadata, not request metadata. It might
include a logical purpose, desired service name, or resumable setup hint, but
the child link still has to perform its own Vox handshake because a `Link` does
not know Vox schemas.

This primitive belongs beside TCP, Unix sockets, WebSocket, FFI, XPC, and
memory links. It should not be a hidden mandatory layer under every Vox
connection.

## Dodeca Topology Case

The existing spec uses Dodeca to justify virtual connections:

```text
Host <-> HTTP Server Cell <-> Browser
```

The browser opens a WebSocket Vox session to the HTTP server cell. The cell
already has a local/FFI Vox session to the host. Today the HTTP server cell
opens a virtual connection on the host session, then proxies the browser
connection to it without translating request IDs or channel IDs.

Dodeca should not force every Vox connection to pay for that topology.

Possible rootless shapes:

- the HTTP server cell asks the host to establish a separate FFI/local link for
  browser devtools, then proxies or hands off that ordinary link;
- the host exposes a local endpoint and the HTTP cell dials it when a browser
  connects;
- the browser connection terminates at the HTTP cell, which forwards at the
  application layer if that is good enough;
- an optional mux carrier is used only if Dodeca truly needs multiple links
  over one host/cell carrier.

The mux version would look like:

```text
Browser
  -> WebSocket link
  -> Vox connection for DevtoolsService

HTTP Server Cell
  -> existing mux carrier to Host
  -> opens child link to Host
  -> Vox connection for DevtoolsService

Proxy
  -> browser link/connection <-> host child link/connection
```

The key property is preserved: the HTTP server cell does not need to understand
or reimplement the Devtools RPC surface if it chooses the proxy shape. It
forwards frames between two links.

The stronger conclusion is not "Dodeca proves core mux is required". It is:
reverse/proxy topologies should be modeled as separate links. A mux carrier is
one possible way to obtain those links, not the core Vox connection model.

## Handshake and Schema Cost

Every produced link should perform a full semantic Vox setup by default:

- transport prologue;
- Vox handshake;
- protocol schema exchange;
- service/schema compatibility check;
- connection settings;
- auth/authorization checks.

That is the correct baseline because the produced link is just a link.

If this is too expensive for high-churn links, optimize with explicit
resumption rather than hidden parent/session state. For example:

- a parent mux carrier or rendezvous protocol can carry a resumption ticket or
  cache identity;
- peers can exchange exact schema digests instead of full schema closures when
  both sides prove they already have the closure;
- digest sets can be sorted and compressed;
- a cache snapshot ID can stand for "the set of schemas we both know";
- any probabilistic summary must be recoverable and cannot be the only source
  of truth.

Bloom filters are not a great first primitive for schema exchange. They have no
false negatives but can have false positives; a false positive would make a
peer think the other side has a schema that it does not have. That can be made
recoverable, but exact digests plus compression are easier to reason about.

## Serving Lifetime

Serving should be driven by an explicit future.

Common shape:

```rust
vox::serve(addr, MyDispatcher::new(service)).await?;
```

More explicit shape:

```rust
let listener = vox::local::bind(path).await?;
let server = vox::Server::new(listener, MyDispatcher::new(service));

server
    .with_graceful_shutdown(shutdown_signal())
    .await?;
```

Dropping an ordinary generated client should not secretly stop an accepted
server peer. Dropping a server future should cancel the server, because the
server future is what owns the work. That matches the Rust pattern used by
Hyper and similar runtimes: drive the serving future until you want it to stop.

This also makes the failure mode more legible. If someone creates a server
future and never awaits/spawns it, the server never runs. That is a simpler
lesson than "you dropped a no-op client that was actually the root liveness
anchor for an accepted session".

Open issue: `Drop` cannot perform graceful async shutdown. Graceful shutdown
must be requested explicitly with a future or handle. Drop can only be abrupt or
best-effort cleanup.

## Graceful Shutdown

Current Vox has close/error behavior, but not a rich drain story.

The rootless model needs protocol language for at least:

- stop accepting new physical links;
- stop opening/accepting new produced links in rendezvous/mux transports;
- stop accepting new requests on a Vox connection;
- let in-flight requests and request-scoped channels finish;
- fail/cancel the remaining work after a deadline;
- report shutdown reason distinctly from peer death.

Tentative terms:

- `retire` means "do not start more work here";
- `drain` means "finish already accepted work";
- `close` means "the transport/protocol is ending now".

For a mux carrier, retire means "do not open or accept new child links". It may
also propagate retire to existing child Vox connections, but those child
connections still need their own drain/close state.

For a Vox connection, retire means "do not send new requests on this service
connection". Existing request scopes and their channels may continue until
they finish or are cancelled.

This area is intentionally not specified yet.

## Auth and Peer Evidence

Authentication and access control cannot be an afterthought because they decide
which links and services may be opened.

The model should be:

```text
transport evidence -> mux/connection peer identity -> service-open auth -> request auth
```

Examples of transport evidence:

- TLS/mTLS peer certificate details;
- ALPN when TLS directly carries Vox frames (`vox/1`) or a wrapped transport
  (`h2`, `http/1.1`, WebSocket);
- Unix socket peer UID/GID/PID where available;
- XPC audit token and code-signing identity on macOS;
- in-process component identity for FFI/shared-library transports;
- synthetic identity for memory/test transports.

This evidence should not be stuffed into ordinary user metadata as if it came
from the remote application. User metadata is application-provided. Transport
evidence is asserted by the local transport. They need different trust levels,
even if the public API lets a server inspect both through one context object.

For mux child links, evidence should be inherited or derived from the parent
carrier unless the mux transport can provide more specific child evidence.

## Observability

This note does not replace
`docs/design/operations-observability-and-progress.md`; it changes the object
model that observability should attach to.

Important consequences:

- transport establishment spans attach to physical links, rendezvous
  mechanisms, or mux carriers;
- produced-link open/accept spans attach to the mechanism that produced the
  link;
- Vox handshake/schema spans attach to each Vox connection;
- request progress attaches to request scopes, not to keepalive or arbitrary
  logs;
- request-scoped channel activity is visible under the request that introduced
  the channel;
- observability/control traffic must not deadlock behind the application lane
  it is trying to explain.

The observability stream may use the same codec and schema machinery as Vox,
but it should not be just another ordinary user request on the endangered
service connection.

## Retry and Reliable Delivery

Retries should not happen below a Vox connection or below a request scope.

Transport/rendezvous/mux/link code can reconnect or re-establish links only
when it has an explicit higher-level operation telling it what is safe to
resume. A raw send failure is not proof that the peer did not observe the
frame.

Raw Vox channels are ordered streams with flow control. They are not durable
queues. Reliable delivery across peer death needs a layer above request scopes:

- operation ID;
- idempotency or replacement semantics;
- per-stream sequence numbers;
- acknowledgement/commit points;
- retention policy;
- replay/resume policy;
- application-visible indeterminate outcomes.

The rootless/mux-link model does not solve reliable delivery by itself, but it
does give retries a cleaner boundary: retry by opening a new Vox connection or
issuing a replacement request scope under the same operation identity, never by
silently replaying link/session frames.

## Tutorial Sketches

Simple client:

```rust
#[vox::service]
trait Catalog {
    async fn lookup(&self, key: String) -> Option<Entry>;
}

let catalog: CatalogClient = vox::connect("local:///tmp/catalog.sock").await?;
let entry = catalog.lookup("facet".to_owned()).await?;
```

Simple server:

```rust
#[tokio::main]
async fn main() -> eyre::Result<()> {
    vox::serve(
        "local:///tmp/catalog.sock",
        CatalogDispatcher::new(CatalogService::new()),
    )
    .await?;

    Ok(())
}
```

Server with explicit graceful shutdown:

```rust
let listener = vox::local::bind("/tmp/catalog.sock").await?;
let server = vox::Server::new(listener, CatalogDispatcher::new(service));

server
    .with_graceful_shutdown(async {
        tokio::signal::ctrl_c().await.ok();
    })
    .await?;
```

Optional mux carrier, one side opens multiple links:

```rust
let carrier = vox::mux::connect("local:///tmp/host.sock").await?;

let catalog: CatalogClient = carrier.connect().await?;
let metrics: MetricsClient = carrier.connect().await?;
```

Optional mux carrier, accepted physical peer also opens a link back:

```rust
let carrier = vox::mux::accept(link).await?;

let serve = carrier.serve(|cx| match cx.service_name() {
    "Worker" => Some(WorkerDispatcher::new(worker.clone()).boxed()),
    _ => None,
});

let control: ControlClient = carrier.connect().await?;

tokio::try_join!(serve, async move {
    control.ready().await?;
    Ok(())
})?;
```

The last example is an advanced transport shape, not the default teaching path.
The important property is more general: accepting one link does not prevent the
acceptor from obtaining another link and opening a Vox connection in the other
logical direction.

## Local User Audit

This was a source/manifests scan under `/Users/amos`, excluding the obvious
cache output when practical. It is not exhaustive proof, but it is enough to
identify the compatibility pressure.

| Checkout | Evidence | Redesign pressure |
| --- | --- | --- |
| `/Users/amos/dodeca` | Uses `NoopClient`, `SessionHandle`, `ConnectionAcceptor`, `open_connection`, and `proxy_connections`. `cells/cell-http/src/devtools.rs` proxies browser Devtools connections through the host. `crates/dodeca/src/cell_loader.rs` stores root sessions for cell links and opens reverse virtual connections. | Important stress case, but not proof that mux belongs in core. May simplify to separate FFI/local links or app-level forwarding. |
| `/Users/amos/stax` | Server accept loops use `.on_connection(...).establish::<vox::NoopClient>()`; clients are mostly simple `vox::connect`. The daemon has custom channel capacity, observer, and keepalive setup. | Root liveness should disappear; advanced server config still needs an explicit server builder/future. |
| `/Users/amos/bee` and `/Users/amos/bee-audio` | Rust FFI/server paths use `.on_connection(...).establish::<vox::NoopClient>()`; Swift app code stores `SessionHandle`; generated TypeScript clients call `established.rootConnection().caller()`. | Cross-language API should remove root handles and generated root access. Swift needs an explicit driven connection/server object. |
| `/Users/amos/dibs` | Example app and service code use `.on_connection(...).establish::<vox::NoopClient>()`; TypeScript generated client uses root connection. | Mostly ordinary service serving and generated-client migration. |
| `/Users/amos/hotmeal` | WASM/browser fuzz paths establish with `NoopClient`; WebSocket links are common. | Browser/WebSocket path should benefit from one link -> one service connection. |
| `/Users/amos/styx` | LSP extension tests and server setup use `.on_connection(dispatcher)`. | Mostly simple server migration. |
| `/Users/amos/vixenware/ccc.vixen.rs` | Backend implements `ConnectionAcceptor` and serves Ccc; client manually establishes TLS/TCP before Vox. | Good auth/evidence case: mTLS/TLS evidence must reach service-open auth. |
| `/Users/amos/vixenware/vixen` | Many Rust paths use `NoopClient`, `.on_connection`, and `vox::serve`; Swift app opens a virtual connection with a VFS dispatcher after a Noop root session; FSKit/local socket code needs local peer identity. | Strong cross-language and local-IPC stress case. May become separate local/XPC/FFI links plus local peer evidence, not necessarily mux. |
| `/Users/amos/helix`, `/Users/amos/helix-fastenc`, `/Users/amos/helix-sched` | Older trace server code uses `vox::serve_listener`; generated web clients use root connection. | Mostly older simple server/client migration, but useful for compatibility shims and examples. |

Compatibility classes:

- Simple generated clients: should become easier. `rootConnection().caller()` and root `Caller` fields disappear from generated public API.
- Simple servers: should become clearer. They drive a server future; no `NoopClient` token.
- Configured servers: still need builder knobs for channel capacity, keepalive, observers, auth/evidence, and graceful shutdown.
- Reverse-service users: need a way to obtain another link in the reverse logical direction. That can be a normal dial, FFI callback, XPC rendezvous, NAT-punching result, or optional mux child link.
- Proxy users: need link-to-link proxy helpers when they choose frame proxying, but application-level forwarding may be better for some products.
- Swift/TypeScript users: need the same conceptual model, without Rust-only drop semantics becoming protocol behavior.

## Migration Sketch

Likely order:

1. Introduce explicit server/connection futures while keeping current protocol.
   Make examples teach "drive the server future" and stop teaching root
   liveness.
2. Define the rootless one-link, one-service connection API and generated
   client/server shapes.
3. Add peer evidence types to accepted links/connections before auth APIs grow
   around untrusted metadata.
4. Remove public reliance on root `NoopClient` in Rust examples and generated
   TypeScript/Swift client shapes.
5. Rework Dodeca and Vixen sketches around separate ordinary links first:
   direct local/FFI/XPC links, explicit rendezvous, or application forwarding.
6. Only introduce a mux carrier abstraction if the real migrations still need
   multiple links over one carrier.
7. Decide whether current virtual connections remain as an internal bridge,
   disappear, or become an optional mux transport implementation detail.
8. Promote surviving semantics into the spec with Tracey requirements.

## Open Questions

- Is a Vox connection always bound to exactly one service surface?
- Is service selection part of the Vox handshake, connection metadata, or both?
- Does an accepted service connection ever initiate requests on the same
  connection, or do all callbacks use another link?
- Is mux needed at all in core, or should it live as a separate transport crate?
- Should NAT traversal be entirely external: a rendezvous/punching protocol that
  returns an ordinary `Link` to Vox?
- Should Dodeca use separate FFI/local links instead of preserving frame-level
  proxying through the HTTP cell?
- If mux exists, does a child link need a stable child-link ID visible to
  observability, or is it transport-private?
- How does graceful retire propagate from external carriers/rendezvous
  mechanisms to produced links and then to request scopes?
- How should request-scoped channel lifetime interact with service-connection
  retire?
- Which evidence fields are portable enough for core Vox, and which belong in
  transport-specific extension structs?
- What exact API shape lets Dodeca choose between app-level forwarding,
  separate-link proxying, or optional mux without making every Vox user pay for
  it?
- Can the first mux transport be implemented over existing Vox virtual
  connections as a migration bridge, or would that preserve too much of the
  root/session model we are trying to remove?
