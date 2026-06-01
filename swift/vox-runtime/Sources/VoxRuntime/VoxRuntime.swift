import Foundation
@preconcurrency import NIOCore

// MARK: - Re-exports for public API

// Encoding
public typealias PostcardEncoder<T> = (T, inout ByteBuffer) -> Void
public typealias PostcardDecoder<T> = (inout ByteBuffer) throws -> T

// `ClientSchemaInfo` now lives in SchemaTracker.swift (phon model).

// MARK: - VoxConnection Protocol

/// Protocol for vox connections (used by generated clients).
public protocol VoxConnection: Sendable {
    /// Make a raw RPC call.
    func call(
        methodId: UInt64,
        metadata: Metadata,
        payload: [UInt8],
        retry: RetryPolicy,
        timeout: TimeInterval?,
        prepareRetry: (@Sendable () async -> PreparedRetryRequest)?,
        finalizeChannels: (@Sendable () -> Void)?,
        schemaInfo: ClientSchemaInfo?
    ) async throws -> [UInt8]

    /// Get the channel allocator.
    var channelAllocator: ChannelIdAllocator { get }

    /// Get the incoming channel registry.
    var incomingChannelRegistry: ChannelRegistry { get }

    /// Get task sender for outgoing channel messages.
    var taskSender: TaskSender { get }
}

extension VoxConnection {
    public func call(
        methodId: UInt64,
        metadata: Metadata,
        payload: [UInt8],
        retry: RetryPolicy,
        timeout: TimeInterval?,
        prepareRetry: (@Sendable () async -> PreparedRetryRequest)?,
        finalizeChannels: (@Sendable () -> Void)?
    ) async throws -> [UInt8] {
        try await call(
            methodId: methodId,
            metadata: metadata,
            payload: payload,
            retry: retry,
            timeout: timeout,
            prepareRetry: prepareRetry,
            finalizeChannels: finalizeChannels,
            schemaInfo: nil
        )
    }

    public func call(
        methodId: UInt64,
        payload: [UInt8],
        retry: RetryPolicy,
        timeout: TimeInterval?
    ) async throws -> [UInt8] {
        try await call(
            methodId: methodId,
            metadata: .null,
            payload: payload,
            retry: retry,
            timeout: timeout,
            prepareRetry: nil,
            finalizeChannels: nil,
            schemaInfo: nil
        )
    }

    public func call(
        methodId: UInt64,
        payload: [UInt8],
        retry: RetryPolicy,
        timeout: TimeInterval?,
        prepareRetry: (@Sendable () async -> PreparedRetryRequest)?,
        finalizeChannels: (@Sendable () -> Void)?
    ) async throws -> [UInt8] {
        try await call(
            methodId: methodId,
            metadata: .null,
            payload: payload,
            retry: retry,
            timeout: timeout,
            prepareRetry: prepareRetry,
            finalizeChannels: finalizeChannels,
            schemaInfo: nil
        )
    }

    public func call(
        methodId: UInt64,
        metadata: Metadata,
        payload: [UInt8],
        timeout: TimeInterval?
    ) async throws -> [UInt8] {
        try await call(
            methodId: methodId,
            metadata: metadata,
            payload: payload,
            retry: .volatile,
            timeout: timeout,
            prepareRetry: nil,
            finalizeChannels: nil,
            schemaInfo: nil
        )
    }

    public func call(methodId: UInt64, payload: [UInt8]) async throws -> [UInt8] {
        try await call(
            methodId: methodId,
            metadata: .null,
            payload: payload,
            retry: .volatile,
            timeout: nil,
            prepareRetry: nil,
            finalizeChannels: nil,
            schemaInfo: nil
        )
    }

    public func call(methodId: UInt64, payload: [UInt8], timeout: TimeInterval?) async throws
        -> [UInt8]
    {
        try await call(
            methodId: methodId,
            metadata: .null,
            payload: payload,
            retry: .volatile,
            timeout: timeout,
            prepareRetry: nil,
            finalizeChannels: nil,
            schemaInfo: nil
        )
    }

}

// MARK: - Connection VoxConnection Conformance

// MARK: - VoxRuntimeError

/// Runtime-originated call errors (the wire error `VoxError<E>` is a separate,
/// generated, generic type the client surfaces). This is what the runtime passes
/// to `ServiceDispatcher.encodeVoxError` to be mapped onto the wire `Err` arm.
///
/// r[impl rpc.fallible.vox-error] - call-level errors.
/// r[impl rpc.error.scope] - Call errors don't terminate connection.
public enum VoxRuntimeError: Error {
    case unknownMethod
    case notImplemented
    case invalidPayload(String)
    case decodeError(String)
    case encodeError(String)
    case connectionClosed
    case timeout
    case cancelled
    case indeterminate
}

// Response/error encoding now goes through the generated dispatcher's phon response
// descriptor (`ServiceDispatcher.encodeVoxError` + the `{service}Methods` table) — the
// hand-rolled postcard byte-literals are gone.

// MARK: - Server-side Channel Helpers

/// Create a server-side Tx for sending to client.
public func createServerTx<T: Sendable>(
    channelId: ChannelId,
    taskSender: @escaping TaskSender,
    registry: ChannelRegistry,
    initialCredit: UInt32,
    serialize: @escaping @Sendable (T, inout ByteBuffer) -> Void
) async -> Tx<T> {
    let tx = Tx<T>(serialize: serialize)
    let credit = await registry.registerOutgoing(channelId, initialCredit: initialCredit)
    tx.bind(channelId: channelId, taskTx: taskSender, credit: credit)
    return tx
}

/// Create a server-side Rx for receiving from client.
public func createServerRx<T: Sendable>(
    channelId: ChannelId,
    receiver: ChannelReceiver,
    deserialize: @escaping @Sendable (inout ByteBuffer) throws -> T
) -> Rx<T> {
    let rx = Rx<T>(deserialize: deserialize)
    rx.bind(channelId: channelId, receiver: receiver)
    return rx
}
