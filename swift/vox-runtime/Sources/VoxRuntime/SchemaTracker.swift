import Foundation
import PhonEngine
import PhonIR
import PhonSchema

// Schema exchange on the phon engine, mirroring TypeScript `vox-core/schema_tracker.ts`.
//
// A peer advertises its type for a (method, direction) binding as a phon schema
// closure (self-describing bytes) in the `schemas:` wire field. The receiver records
// the writer closure and builds a compatibility decode program reconciling it against
// the local reader DESCRIPTOR (the Swift typed path needs the reader's memory layout —
// supplied by codegen — not just a reader root). Field matching/reordering/defaulting
// is phon's `lowerDecode`. `r[impl schema.tracking.received]`

/// Args vs. response binding direction (the generated `BindingDirection` wire enum
/// is the on-wire form; this is the tracker's local key).
public enum SchemaBindingDirection: Sendable, Hashable {
    case args
    case response
}

/// A channel argument's position + direction + element root + the element's phon
/// schema-closure bytes (so the element's `elementRoot` resolves in the service
/// registry and the typed element codec can be built). Emitted by codegen.
public struct PhonChannelMeta: Sendable {
    public let index: Int
    public let isTx: Bool
    public let elementRoot: SchemaId
    public let elementSchemaClosure: [UInt8]
    public init(index: Int, isTx: Bool, elementRoot: SchemaId, elementSchemaClosure: [UInt8] = []) {
        self.index = index
        self.isTx = isTx
        self.elementRoot = elementRoot
        self.elementSchemaClosure = elementSchemaClosure
    }
}

/// Per-method schema data emitted by vox-codegen (`{service}Methods`). Carries both
/// the content roots + closures (for advertising / reconciliation) and the Swift
/// typed-path descriptors (for the concrete memory decode).
public struct PhonMethodSchemas: @unchecked Sendable {
    public let argsRoot: SchemaId
    public let argsSchemaClosure: [UInt8]
    public let argsDescriptor: Descriptor
    /// Recursion blocks for the args descriptor's cyclic schemas (`[:]` when none) —
    /// the `Access.recurse` stand-ins resolve into these via `CallBlock`.
    public let argsDescriptorBlocks: [SchemaId: Descriptor]
    public let okRoot: SchemaId
    /// Root of the response wire type `Result<T, VoxError<E>>` (server encode).
    public let responseRoot: SchemaId
    public let responseSchemaClosure: [UInt8]
    public let responseDescriptor: Descriptor
    /// Recursion blocks for the response descriptor's cyclic schemas (`[:]` when none).
    public let responseDescriptorBlocks: [SchemaId: Descriptor]
    public let channels: [PhonChannelMeta]

    public init(
        argsRoot: SchemaId, argsSchemaClosure: [UInt8], argsDescriptor: Descriptor,
        argsDescriptorBlocks: [SchemaId: Descriptor] = [:],
        okRoot: SchemaId, responseRoot: SchemaId, responseSchemaClosure: [UInt8],
        responseDescriptor: Descriptor, responseDescriptorBlocks: [SchemaId: Descriptor] = [:],
        channels: [PhonChannelMeta] = []
    ) {
        self.argsRoot = argsRoot
        self.argsSchemaClosure = argsSchemaClosure
        self.argsDescriptor = argsDescriptor
        self.argsDescriptorBlocks = argsDescriptorBlocks
        self.okRoot = okRoot
        self.responseRoot = responseRoot
        self.responseSchemaClosure = responseSchemaClosure
        self.responseDescriptor = responseDescriptor
        self.responseDescriptorBlocks = responseDescriptorBlocks
        self.channels = channels
    }
}

/// A service's phon registry + per-method schemas (the `ServiceDescriptor.send_schemas`
/// + `registry` of TS). Emitted by codegen.
public struct ServiceSchemas: @unchecked Sendable {
    public let registry: Registry
    public let methods: [UInt64: PhonMethodSchemas]
    public init(registry: Registry, methods: [UInt64: PhonMethodSchemas]) {
        self.registry = registry
        self.methods = methods
    }
}

/// Schema information for a single client call (replaces the old vox-schema
/// `ClientSchemaInfo`).
public struct ClientSchemaInfo: @unchecked Sendable {
    public let methodSchemas: PhonMethodSchemas
    public let registry: Registry
    public init(methodSchemas: PhonMethodSchemas, registry: Registry) {
        self.methodSchemas = methodSchemas
        self.registry = registry
    }
}

/// Build a service registry by merging every method's args + response schema
/// closures (deduped by id in `Registry`). Used by codegen's `{service}Registry`.
public func buildServiceRegistry(_ methods: [UInt64: PhonMethodSchemas]) -> Registry {
    var schemas: [Schema] = []
    for m in methods.values {
        if let a = try? parseSchemaClosure(m.argsSchemaClosure) { schemas += a.schemas }
        if let r = try? parseSchemaClosure(m.responseSchemaClosure) { schemas += r.schemas }
        // Channel element schemas: the args/response closures don't reference the element
        // type (a channel arg is a u32 wire index on the wire), so merge them explicitly
        // — the typed element codec resolves `elementRoot` against this registry.
        for ch in m.channels {
            if let e = try? parseSchemaClosure(ch.elementSchemaClosure) { schemas += e.schemas }
        }
    }
    return Registry(schemas)
}

private struct BindingKey: Hashable {
    let methodId: UInt64
    let direction: SchemaBindingDirection
}

/// Tracks the writer schema closures a peer advertised, and builds compat decode
/// programs against local reader descriptors.
public final class SchemaTracker: @unchecked Sendable {
    private var received: [BindingKey: (root: SchemaId, schemas: [Schema])] = [:]
    /// Cache of the reconciling programs (planning is amortized: built once per writer,
    /// reused for every decode). Invalidated when a binding's writer is re-advertised.
    private var programs: [BindingKey: Lowered] = [:]
    private let lock = NSLock()

    public init() {}

    public func reset() {
        lock.lock(); defer { lock.unlock() }
        received.removeAll()
        programs.removeAll()
    }

    /// Record the peer's phon schema-closure bytes for a binding (best-effort,
    /// idempotent: a later advertisement overwrites and drops the cached program).
    public func recordReceived(_ methodId: UInt64, _ direction: SchemaBindingDirection, _ schemaBytes: [UInt8]) {
        guard !schemaBytes.isEmpty, let bundle = try? parseSchemaClosure(schemaBytes) else { return }
        let key = BindingKey(methodId: methodId, direction: direction)
        lock.lock(); defer { lock.unlock() }
        received[key] = (bundle.root, bundle.schemas)
        programs[key] = nil
    }

    public func hasReceived(_ methodId: UInt64, _ direction: SchemaBindingDirection) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return received[BindingKey(methodId: methodId, direction: direction)] != nil
    }

    /// The reconciling decode program for `(methodId, direction)` producing the reader
    /// type described by `readerDescriptor`, resolved through `local` + the writer's
    /// exchanged schemas — `lowerDecode(writer → reader)`, the ONLY decode path. Built
    /// once and cached. Returns nil only when no writer schema was advertised (a
    /// protocol error for the caller to surface — never a same-schema fallback).
    public func buildDecodeProgram(
        _ methodId: UInt64, _ direction: SchemaBindingDirection,
        readerDescriptor: Descriptor, readerBlocks: [SchemaId: Descriptor] = [:], local: Registry
    ) -> Lowered? {
        let key = BindingKey(methodId: methodId, direction: direction)
        lock.lock(); defer { lock.unlock() }
        if let cached = programs[key] { return cached }
        guard let writer = received[key],
            let program = try? lowerDecode(
                writer.root, readerDescriptor, local.with(writer.schemas), readerBlocks)
        else { return nil }
        programs[key] = program
        return program
    }
}

/// Tracks which (method, direction) schema closures have been advertised on a
/// connection, so each is sent at most once (`r[schema.exchange.idempotent]`).
public final class SchemaSendTracker: @unchecked Sendable {
    private var sent: Set<BindingKey> = []
    private let lock = NSLock()

    public init() {}

    public func reset() {
        lock.lock(); defer { lock.unlock() }
        sent.removeAll()
    }

    /// The phon schema-closure bytes to advertise for `(methodId, direction)`, or `[]`
    /// when already sent.
    public func prepareSchemas(_ methodId: UInt64, _ direction: SchemaBindingDirection, _ closure: [UInt8]) -> [UInt8] {
        let key = BindingKey(methodId: methodId, direction: direction)
        lock.lock(); defer { lock.unlock() }
        if sent.contains(key) { return [] }
        sent.insert(key)
        return closure
    }
}
