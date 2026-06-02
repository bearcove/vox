import Foundation
import PhonSchema

public final class Connection: @unchecked Sendable {
    let handle: ConnectionHandle
    /// Writer schema closures the peer advertised — the generated client reconciles the
    /// response decode against the server's advertised response schema through this.
    public let schemaReceiveTracker: SchemaTracker

    init(handle: ConnectionHandle, schemaReceiveTracker: SchemaTracker) {
        self.handle = handle
        self.schemaReceiveTracker = schemaReceiveTracker
    }

    public var channelAllocator: ChannelIdAllocator {
        handle.channelAllocator
    }

    public var incomingChannelRegistry: ChannelRegistry {
        handle.channelRegistry
    }

    public var taskSender: TaskSender {
        { [weak self] msg in
            self?.sendTaskMessage(msg)
        }
    }

    public func call(
        methodId: UInt64,
        metadata: Metadata,
        payload: [UInt8],
        retry: RetryPolicy = .volatile,
        timeout: TimeInterval?,
        prepareRetry: (@Sendable () async -> PreparedRetryRequest)? = nil,
        finalizeChannels: (@Sendable () -> Void)? = nil,
        schemaInfo: ClientSchemaInfo? = nil
    ) async throws -> [UInt8] {
        try await callRaw(
            methodId: methodId,
            metadata: metadata,
            payload: payload,
            retry: retry,
            timeout: timeout,
            prepareRetry: prepareRetry,
            finalizeChannels: finalizeChannels,
            schemaInfo: schemaInfo
        )
    }

    public func callRaw(
        methodId: UInt64,
        metadata: Metadata = .null,
        payload: [UInt8],
        retry: RetryPolicy = .volatile,
        timeout: TimeInterval? = nil,
        prepareRetry: (@Sendable () async -> PreparedRetryRequest)? = nil,
        finalizeChannels: (@Sendable () -> Void)? = nil,
        schemaInfo: ClientSchemaInfo? = nil
    ) async throws -> [UInt8] {
        try await handle.callRaw(
            methodId: methodId,
            metadata: metadata,
            payload: payload,
            retry: retry,
            timeout: timeout,
            prepareRetry: prepareRetry,
            finalizeChannels: finalizeChannels,
            schemaInfo: schemaInfo
        )
    }

    public func sendTaskMessage(_ msg: TaskMessage) {
        handle.sendTaskMessage(msg)
    }
}

extension Connection: VoxConnection {}
