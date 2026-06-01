/// Protocol for dispatching incoming requests.
public protocol ServiceDispatcher: Sendable {
    func retryPolicy(methodId: UInt64) -> RetryPolicy

    /// Encode a runtime-originated `VoxRuntimeError` (cancelled, indeterminate,
    /// invalid payload, …) as a response payload. The wire type is
    /// `Result<T, VoxError<E>>`, whose `Err` arm is independent of the method's
    /// `T`/`E`, so the generated dispatcher encodes it through any method's response
    /// descriptor (mirrors TS `encodeVoxError`).
    func encodeVoxError(_ error: VoxRuntimeError) -> [UInt8]

    /// Pre-register any channels in the request payload.
    /// This is called synchronously BEFORE spawning the handler task,
    /// ensuring channels are registered before any Data messages arrive.
    func preregister(
        methodId: UInt64,
        payload: [UInt8],
        registry: ChannelRegistry
    ) async

    /// Dispatch a request. Called in a spawned task after preregister.
    func dispatch(
        methodId: UInt64,
        payload: [UInt8],
        requestId: UInt64,
        registry: ChannelRegistry,
        schemaSendTracker: SchemaSendTracker,
        schemaReceiveTracker: SchemaTracker,
        taskTx: @escaping @Sendable (TaskMessage) -> Void
    ) async
}

public extension ServiceDispatcher {
    func retryPolicy(methodId _: UInt64) -> RetryPolicy {
        .volatile
    }
}
