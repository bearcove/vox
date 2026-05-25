+++
title = "Schema Exchange"
description = "Backwards-compatible type evolution for compact Binette values"
weight = 15
+++

Compact Binette is Vox's steady-state data format. Compact Binette bytes are
schema-driven: fields are identified by their order in the sender schema, not
by name in the byte stream. This means that adding, removing, or reordering
fields changes the byte layout, and a peer reading with a different type
definition would silently misinterpret the data without Vox schema exchange.

Schema exchange solves this without making compact values self-describing.
Peers describe their types to each other by sending Binette schema bundles
encoded as self-describing Binette values. Vox adds the exchange rules,
per-connection tracking, method binding, and channel attachment rules needed
for RPC.

The result: compact Binette remains the fast path for serialization and
deserialization, but peers with different versions of the same types can
communicate safely. Vox applies Binette compatibility planning before payload
decode so incompatibilities are detected early, not mid-stream when a field has
the wrong value.

# Design principles

> r[schema.principles.no-roundtrips]
>
> Schema exchange MUST NOT require request-response negotiation. The sender
> proactively includes schemas before data when the receiver has not seen
> them. No round trips, no handshake, no "do you have this schema?" queries.

> r[schema.principles.sender-driven]
>
> Each peer tracks which schemas it has sent to the other side. When a peer
> is about to send data of a type the other side has not seen, it sends the
> schema first. The receiver never requests schemas — the sender pushes them.

> r[schema.principles.self-describing]
>
> Schema payloads MUST be encoded using self-describing Binette. Self-describing
> mode does not require a schema to parse, avoiding the chicken-and-egg problem
> of needing a schema to read a schema. Compact Binette is used for data;
> self-describing Binette is used for schema bundles and other metadata about
> data.

> r[schema.principles.once-per-type]
>
> A schema for a given type ID MUST be sent at most once per connection.
> Once a peer has sent a schema, it records the type ID as "sent" and does
> not send it again for the lifetime of the connection.

# Binette schema use

Binette owns the value model, schema model, type IDs, schema bundles, schema
compatibility rules, and generic external attachment mechanism. Vox does not
define a second schema language. Vox uses Binette schema bundles and adds only
RPC-specific delivery, per-connection tracking, and method binding rules.

Vox's `TypeSchemaId` is the Binette `u64` content hash. Content hashes give
type IDs a universal meaning: a peer that receives a schema tagged with a
content hash it has already seen — from this connection, a previous connection,
or a persistent store — knows it already has that schema. This is critical for
operation stores (see `r[schema.interaction.retry]`) and for efficient schema
tracking across connection resumes.

> r[schema.type-id.hash.channel+3]
>
> Vox MUST NOT define a separate channel type-ID hash. Channel endpoints are
> represented with Binette's external-attachment schema kind using external
> kind string `"vox.channel"`. The channel endpoint's type ID is the Binette
> type ID of that external-attachment schema.
>
> The `"vox.channel"` external metadata contains:
>
> - `direction`: `"send"` or `"recv"`
> - `element`: the channel item type reference
>
> This makes channel positions visible to the Binette schema walk while keeping
> channel allocation, flow control, and attachment IDs in Vox.

> r[schema.type-id.per-connection]
>
> Every connection starts with zero schema knowledge. A peer MUST NOT
> assume that schemas sent on one connection are available on another,
> even within the same session. Each connection half has its own
> sent/received tracking. However, because type IDs are content hashes,
> a peer MAY use a previously received schema (from another connection
> or a persistent cache) to build a translation plan without waiting for
> the schema to be resent — as long as it does not send data until the
> remote peer has confirmed (by sending its own schemas) that it can
> read it.

Per-connection tracking is required because connections within a session
may terminate at different peers. Consider this topology:

```
     B  → C  (Conn 0 aka root connection)
A ← (B) ← C  (Conn 1)
```

Connection 0 (root) between B and C serves one set of services. C
requests a virtual connection (ID 1), which B forwards to A. B routes
`MessagePayload`s for connection 1 between A and C without inspecting
their content — B does not know what services A and C are speaking on
that connection, and does not need to.

If schema knowledge leaked across connections — for example, if a peer
assumed "I already sent `String`'s schema on connection 0, so I don't
need to send it on connection 1" — the proxy would break. A never saw
connection 0's schemas; it only sees connection 1. Each connection is
an independent communication channel that may reach a different peer,
so schema state must be tracked independently per connection.

# Vox schema payload format

A Vox schema payload carries a Binette schema bundle. The bundle root is the
application type bound to one `(method_id, direction)` pair.

```rust
struct TypeSchemaId(u64);

/// A Binette type reference, encoded as specified by Binette.
struct TypeRef;

/// The direction of a channel endpoint.
enum ChannelDirection {
    Send,
    Recv,
}

/// A Binette schema bundle as specified by Binette.
struct BinetteSchemaBundle;
```

Generic types are sent once per declaration, not once per instantiation.
For example, `Result<u32, MyError>` and `Result<String, OtherError>` both
reference the same `Result` schema — only the type arguments at the use
site differ. This is more efficient on the wire and enables cross-language
matching where different languages may format generic type names differently.

Container types (`list`, `set`, `map`, `array`, `option`) are built-in generics.
Their element/key/value references use type references like any other schema
reference, but they do not need explicit `type_params` because their
generic structure is implicit in the Binette schema kind.

The normative rules below define the self-describing Binette encoding of
Vox schema payloads.

> r[schema.format+4]
>
> A Vox `SchemaPayload` MUST be a self-described Binette struct containing a
> `bundle` field. The `bundle` field is a Binette schema bundle whose root is
> the type bound by this `SchemaMessage`.

> r[schema.format.type-id]
>
> A `TypeSchemaId` MUST be encoded as a `u64`.

> r[schema.format.type-ref+4]
>
> Vox type references use the Binette type-reference encoding. Vox does not add
> a Vox-specific type-reference variant.

> r[schema.format.primitive+3]
>
> Vox primitive schemas are Binette primitive schemas. Vox does not define
> primitive schema metadata.

> r[schema.format.struct+4]
>
> Vox struct schemas are Binette struct schemas. Producer-side field
> defaultability is not sent in live Vox schema exchange; default providers are
> reader-side information used by the Binette compatibility rules.

> r[schema.format.enum+4]
>
> Vox enum schemas are Binette enum schemas. Vox does not define enum schema
> metadata.

> r[schema.format.container+4]
>
> Vox container schemas are Binette container schemas. Lists and sets remain
> distinct schema kinds as specified by Binette.

> r[schema.format.tuple+4]
>
> Vox tuple schemas are Binette tuple schemas.

> r[schema.format.channel+4]
>
> Vox channel endpoints MUST be represented as Binette external attachments
> using the `"vox.channel"` external kind from
> `r[schema.type-id.hash.channel+3]`. The in-band compact value is Binette
> unit. The actual channel ID is carried out-of-band in the message's
> `channels` field.

## Recursive types on the wire

Recursive types reference each other by their final `TypeSchemaId` — the
same plain `u64` content hash as any other type. There is no special
wire representation for recursive references. The schemas for all
types in a recursive group simply reference each other by hash.

> r[schema.format.recursive+3]
>
> Recursive schemas are represented using Binette recursive type IDs and
> Binette schema bundles. When sending schemas for a recursive group, the
> sender MUST include all schemas in the group that have not already been sent
> on this connection.

## Schema delivery

Application-level schemas are sent as standalone `SchemaMessage`
frames. Each frame introduces exactly one `(method_id, direction)`
binding and carries the root type for that binding plus any newly
introduced schemas the receiver needs before it can deserialize the
subsequent `Request` or `Response`.

```rust
enum BindingDirection {
    Args,
    Response,
}

/// The self-describing Binette payload carried by a SchemaMessage.
struct SchemaPayload {
    /// The Binette schema bundle for this method binding.
    bundle: BinetteSchemaBundle,
}
```

> r[schema.format.self-contained]
>
> When a `SchemaMessage` includes schemas, the set of schemas MUST be
> self-contained. Every `TypeSchemaId` referenced by any schema in the set
> MUST either be defined in the same set or have been previously sent on
> this connection. The receiver MUST be able to build translation plans for
> all included types before deserializing the payload.

> r[schema.format.id-verified+4]
>
> Before installing schemas from a `SchemaPayload`, the receiver MUST verify the
> Binette schema bundle using Binette schema registry rules. A schema whose
> declared ID does not match its content, whose bundle is not self-contained, or
> whose root cannot be resolved is a protocol error. The receiver MUST NOT
> install any schema from a rejected payload in the connection's schema registry.

> r[schema.format.delivery+3]
>
> Application-level schemas are delivered as a standalone `SchemaMessage`
> containing a self-describing Binette `SchemaPayload`. The payload MUST
> include:
>
>   * a Binette schema bundle containing all schemas needed for the method's
>     types that have not been previously sent on this connection
>   * the bundle root type reference for one `(method_id, direction)` binding
>
> The root type for a response is always the full
> `Result<T, VoxError<E>>` wire type, regardless of whether the
> handler succeeded or failed.
>
> A `SchemaMessage` binds exactly one `(method_id, direction)` pair. If all
> schemas for that method's types have already been sent on this connection,
> the bundle's `schemas` list MAY be empty — but the binding message MUST still be sent
> the first time this `(method_id, direction)` pair is introduced on the
> connection. The receiver needs the binding to know which previously-sent
> `TypeSchemaId` is the root for this method. Sending a schema whose
> `TypeSchemaId` has already been sent on this connection is a protocol error.

# Schema tracking

Each peer maintains two sets per connection:

> r[schema.tracking.sent]
>
> Each peer MUST track the set of type IDs for which it has sent schemas to
> the other peer. This set starts empty and grows monotonically over the
> connection lifetime.

> r[schema.tracking.received]
>
> Each peer MUST track the set of type IDs for which it has received schemas
> from the other peer. This set starts empty and grows monotonically over
> the connection lifetime.

> r[schema.tracking.transitive+3]
>
> When a schema bundle is sent, all type IDs transitively referenced by that
> bundle are also marked as sent. A schema payload is self-contained
> (see `r[schema.format.self-contained]`), so sending a Binette bundle
> implicitly sends the schemas reachable from its root.

> r[schema.tracking.bindings]
>
> Each peer MUST track the set of (method_id, direction) pairs for which
> it has sent method bindings on this connection. A binding MUST be sent
> the first time a method's schemas are delivered for a given direction,
> even if all the schemas themselves were already sent by a previous call
> to a different method.

# Two levels of schema exchange

Schema exchange operates at two levels:

1. **Protocol level (per-session):** The `MessagePayload` schema is
   exchanged during the self-describing Binette handshake
   (see `r[session.handshake]`).
   This allows the protocol framing itself to evolve without breaking
   changes.

2. **Application level (per-connection):** Method argument and response
   schemas are exchanged lazily via `SchemaMessage`, scoped to each
   connection. This allows service types to
   evolve independently.

The rest of this section describes application-level schema exchange.

# When schemas are exchanged

Schema exchange is triggered by method invocation. The caller sends schemas
for its argument types; the callee sends schemas for its response types. This
is lazy — schemas are only exchanged for types actually used in calls, not
for the entire service interface up front.

> r[schema.exchange.caller+2]
>
> Before sending a `Request`, the caller MUST check whether the schemas for
> the method's argument types have been sent to this peer on this connection.
> If any have not, the caller MUST send a `SchemaMessage` carrying all unsent
> schemas and the method binding before sending the `Request`
> (see `r[schema.format.delivery+3]`).

> r[schema.exchange.callee]
>
> Before sending any `Response` for a method, the callee MUST check whether
> the schemas for the method's **statically-known response type** have been
> sent to this peer on this connection. If any have not, the callee MUST
> send a `SchemaMessage` carrying all unsent schemas and the method binding
> before sending the `Response`.
>
> The response schema is determined by the method signature — it is the
> full `Result<T, VoxError<E>>` wire type. It MUST NOT vary based on
> whether the handler succeeded or failed. Sending schemas for a different
> type (e.g. `Result<(), VoxError<E>>` when the method returns
> `Result<T, VoxError<E>>`) is a protocol error.

> r[schema.exchange.channels]
>
> Channel endpoint schemas are included in schema exchange using the
> `"vox.channel"` Binette external-attachment kind. If a method's arguments
> contain `Tx<T>` or `Rx<T>`, the external attachment metadata identifies the
> endpoint direction and the schema for `T` MUST be included in the caller's
> bundle. Channels MUST NOT appear in return types (see
> `r[rpc.channel.placement]`).

> r[schema.exchange.required]
>
> Application-level schema exchange is mandatory. If a peer receives a
> `Request` or `Response` and either (a) the schemas for any referenced
> type have not been received on that connection, or (b) no
> method binding for this `(method_id, direction)` pair has been
> received on this connection, this is a protocol error and the
> connection MUST be torn down. The sender is always responsible for
> sending both schemas and bindings before the data that needs them.

> r[schema.exchange.idempotent]
>
> If the caller has already sent schemas for a method's argument types
> (from a previous call to the same or different method using the same
> types), no schemas need to be included. The `r[schema.principles.once-per-type]`
> rule applies — each type ID is sent at most once. However, the
> binding for a new `(method_id, direction)` pair MUST still
> be sent in its own `SchemaMessage` even when all schemas are already known
> (see `r[schema.tracking.bindings]`).

# Method identity without signatures

Schema exchange is mandatory (see `r[session.handshake]`). Since peers
always have each other's method-bound schema roots, method identity no longer
needs to encode the full type signature. Two versions of a service may have
the same method with evolved argument types — including the signature
hash in the method ID would make these look like different methods,
which is exactly what schema exchange is designed to avoid.

> r[schema.method-id]
>
> The method ID MUST be computed as:
> ```
> method_id = blake3(kebab(ServiceName) + "." + kebab(methodName))[0..8]
> ```
> The signature hash (`sig_bytes` from `r[signature.hash.algorithm]`) is
> excluded. Only the service name and method name contribute to the method
> ID.

Renaming a method is still a breaking change (the method ID changes),
but changing argument or return types is no longer automatically
breaking — it depends on whether the translation plan can bridge the
difference.

# Translation plans

When a peer receives a schema for a remote type that it will deserialize
into a local type, it builds a **translation plan** using Binette compatibility
rules. The translation plan is a recipe for reading compact Binette bytes
written by the remote type and populating the fields of the local type.

Translation plans are built once per (remote type ID, local type) pair
and cached for the connection lifetime.

> r[schema.translation.field-matching]
>
> Fields are matched by name, not by position. Vox's plan-builder applies
> Binette field-matching compatibility when constructing a translation plan
> from a remote schema and a local type.

> r[schema.translation.skip-unknown+2]
>
> Fields present in the remote schema but absent from the local type MUST be
> skipped during deserialization. Vox uses Binette's schema-driven skip rule:
> the translation plan skips an unmatched value by walking the sender schema
> for that value. Vox's plan-builder records "skip" entries for unmatched remote
> fields when it constructs the plan.

> r[schema.translation.fill-defaults]
>
> Local fields absent from the remote schema MUST be filled with their
> default values. Whether a local field has a default is reader-side
> defaultability information as defined by Binette compatibility tooling; Vox
> live schema exchange does not carry producer-side `required` flags. Local
> fields without a default that are missing from the remote schema cause a
> translation plan error (see `r[schema.errors.missing-required]`).

> r[schema.translation.reorder]
>
> When fields exist in both schemas but in different declaration order, the
> translation plan MUST reorder during decode. This is a direct consequence of
> `r[schema.translation.field-matching]`: the plan remaps remote positions to
> local field offsets, then the compiled decoder writes each remote field into
> its local slot.

> r[schema.translation.type-compat]
>
> For each matched field, the remote field type and local field type MUST be
> compatible according to Binette type-compatibility rules. Vox does not add
> integer widening or other RPC-specific coercions.

> r[schema.translation.serialization-unchanged]
>
> Schema exchange does NOT affect serialization. A peer always serializes
> using its own local type definition and compact Binette. The translation
> plan applies only on the deserialization side — the receiver adapts to the
> sender's layout.

# Enum evolution

Enums follow the same principle as structs — match by name, not by position.
This allows adding variants to an enum without breaking existing peers.

> r[schema.translation.enum]
>
> Enum variants are matched by name, not by variant index. Vox's plan-builder
> applies Binette enum compatibility, maps remote variant names to local variant
> indices, and records how to deserialize each variant's payload.

> r[schema.translation.enum.unknown-variant]
>
> If a remote enum has variants that the local type does not, those variants
> are skippable in the schema but cause an error at runtime if actually
> received. The translation plan records that these variants exist in the
> remote schema; if a message arrives with an unknown variant, the
> deserializer MUST return an error.

> r[schema.translation.enum.missing-variant]
>
> If the local enum has variants that the remote schema does not, this is
> fine — those variants will never appear in data from that remote peer.
> No error is needed. The local peer can still use those variants when
> sending data.

> r[schema.translation.enum.payload-compat]
>
> For each variant that exists in both the remote and local types, the
> variant payloads MUST be compatible: unit matches unit, newtype matches
> newtype with a compatible inner type, tuple matches tuple with
> compatible elements (see `r[schema.translation.tuple]`), struct matches
> struct with compatible fields (same rules as top-level struct matching).

# Tuple evolution

Tuples are positional — elements are matched by index, not by name.
This means tuple evolution is more restricted than struct evolution.

> r[schema.translation.tuple]
>
> Tuple types MUST be compatible according to Binette tuple-compatibility rules.
> Vox does not add tuple-specific adaptation beyond the Binette plan.

# Error reporting

Schema exchange detects incompatibilities early — when building the
translation plan — rather than failing mid-stream on corrupt data.

> r[schema.errors.early-detection]
>
> Type incompatibilities MUST be detected at translation-plan construction
> time, not during deserialization of individual messages. When a peer
> receives a schema and attempts to build a translation plan against a
> local type, all structural incompatibilities MUST be reported before
> any data of that type is processed.

> r[schema.errors.call-level]
>
> A translation plan failure is a **call-level error**, not a connection-level
> fault. The connection remains open and other method calls are unaffected.
> This is distinct from missing schemas entirely (a protocol error per
> `r[schema.exchange.required]`), which tears down the connection.

> r[schema.errors.call-level.callee]
>
> If the callee cannot build a translation plan for incoming request
> arguments, it MUST respond with an error describing the incompatibility
> (including a diff of the remote schema versus the local type).

> r[schema.errors.call-level.caller]
>
> If the caller cannot build a translation plan for an incoming response,
> the failure is local — the call's result resolves to an error. There is
> no further message to send; the response has already been received.

> r[schema.errors.non-retryable]
>
> A translation plan failure is non-retryable for the life of the connection to
> that remote peer. The remote peer's schema for a given type does not change
> while the connection is open, so retrying the same call will always reproduce
> the same translation plan failure. Callers MUST treat a translation plan
> failure as non-retryable (see `r[rpc.fallible.vox-error.retryable]`).

> r[schema.errors.missing-required]
>
> If a local struct has a field without a default value that is not
> present in the remote schema, the translation plan MUST fail with an
> error identifying the missing field by name and type. Whether a field
> has a default is a property of the local type definition, not of the
> remote schema (see `r[schema.translation.fill-defaults]`).

> r[schema.errors.type-mismatch]
>
> If a field exists in both the remote and local types but the types are
> incompatible (e.g., remote has `u32`, local has `String`), the
> translation plan MUST fail with an error identifying the field, the
> remote type, and the local type.

> r[schema.errors.unknown-variant-runtime]
>
> If a message arrives containing an enum variant that exists in the
> remote schema but not in the local type, the deserializer MUST return
> an error for that specific message. This is a runtime error because
> the translation plan cannot predict which variant a given message
> will contain.

> r[schema.errors.content]
>
> All schema-related errors MUST include:
>
>   * The remote type ID
>   * The local type name (for diagnostics)
>   * The specific incompatibility (missing field, type mismatch, etc.)
>   * For field-level errors: the field name and both the remote and local
>     field types

# Compatibility checking

Schema exchange handles runtime differences gracefully, but it is still
valuable to know about compatibility issues before deployment. Tooling
can snapshot schemas and check changes as part of the development workflow.

> r[schema.compat.snapshot]
>
> Implementations SHOULD provide tooling to snapshot the Binette schema bundles
> for a service's method bindings. A snapshot captures every schema reachable
> from the service's method argument and response roots.

> r[schema.compat.check+2]
>
> Implementations SHOULD provide tooling to compare two snapshots using Binette
> compatibility rules and report:
>
>   * **Compatible changes** — changes where a translation plan can be
>     built in both directions (e.g., adding a field with a default)
>   * **One-way compatible changes** — changes where old can read new but
>     not vice versa (e.g., adding a field without a default)
>   * **Breaking changes** — changes where no translation plan can be
>     built (e.g., removing a field that local readers cannot default-fill,
>     changing a field's type incompatibly)

> r[schema.compat.ci]
>
> Schema compatibility checks SHOULD be integrated into CI pipelines.
> Breaking changes should fail the build unless explicitly acknowledged.

> r[schema.compat.policy]
>
> A breaking change is one where a Binette translation plan cannot be built
> between the old and new versions. Whether a breaking change is acceptable
> depends on the project's deployment model (rolling updates vs. coordinated
> releases). The tooling reports facts; policy is up to the project.

# Interaction with other spec areas

Schema exchange is designed to be transparent to the rest of the protocol.

> r[schema.interaction.channels]
>
> Channel semantics (creation, flow control, close, reset) are unchanged by
> schema exchange. Channel endpoints appear in Binette schemas as
> `"vox.channel"` external attachments (see `r[schema.exchange.channels]`), and
> translation plans apply to channel items the same way they apply to
> request/response payloads.

> r[schema.interaction.retry]
>
> Operation stores MUST store schemas alongside serialized payloads.
> A sealed operation contains compact Binette bytes that are only
> meaningful together with the schemas that describe them. When replaying
> a sealed response, the replaying peer MUST send schemas for the
> response types on the current connection if they have not already been
> sent, just as it would for a live response.
>
> Because type IDs are content hashes, the operation store does not need
> a per-connection schema ID namespace. The stored schemas use the same
> content hashes regardless of which connection originally produced them
> or which connection replays them. A disk-backed operation store that
> survives process restarts can use content hashes as stable keys for
> its schema cache.

> r[schema.interaction.metadata]
>
> Metadata is unaffected by schema exchange. Metadata key-value pairs are
> not typed in the compact-wire sense and do not participate in schema exchange.
