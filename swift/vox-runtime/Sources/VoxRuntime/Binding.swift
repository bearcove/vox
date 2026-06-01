import Foundation
@preconcurrency import NIOCore

// MARK: - Unbound Channel Types

/// Unbound Tx - created by `channel()`, bound at call time.
public final class UnboundTx<T: Sendable>: @unchecked Sendable {
    public private(set) var channelId: ChannelId = 0
    private var taskTx: (@Sendable (TaskMessage) -> Void)?
    private var credit: ChannelCreditController?
    private let serialize: @Sendable (T, inout ByteBuffer) -> Void
    private var bound = false
    private var closed = false
    private let lock = NSLock()
    private var bindingWaiters: [CheckedContinuation<Void, Never>] = []
    weak var pairedRx: AnyObject?

    public init(serialize: @escaping @Sendable (T, inout ByteBuffer) -> Void) {
        self.serialize = serialize
    }

    public var isBound: Bool { bound }

    /// Bind for sending (client-side outgoing).
    func bind(
        channelId: ChannelId,
        taskTx: @escaping @Sendable (TaskMessage) -> Void,
        credit: ChannelCreditController
    ) {
        let (waiters, shouldCloseImmediately) = lock.withLock {
            () -> ([CheckedContinuation<Void, Never>], Bool) in
            self.channelId = channelId
            self.taskTx = taskTx
            self.credit = credit
            self.bound = true
            let shouldCloseImmediately = self.closed
            self.closed = false
            let waiters = self.bindingWaiters
            self.bindingWaiters.removeAll()
            return (waiters, shouldCloseImmediately)
        }
        for waiter in waiters {
            waiter.resume()
        }
        if shouldCloseImmediately {
            Task {
                await credit.close()
            }
            taskTx(.close(channelId: channelId))
        }
    }

    /// Set channel ID only (when paired Rx is bound).
    func setChannelIdOnly(channelId: ChannelId) {
        let waiters = lock.withLock { () -> [CheckedContinuation<Void, Never>] in
            self.channelId = channelId
            self.bound = true
            let waiters = self.bindingWaiters
            self.bindingWaiters.removeAll()
            return waiters
        }
        for waiter in waiters {
            waiter.resume()
        }
    }

    /// Send a value.
    public func send(_ value: T) async throws {
        let (taskTx, credit) = try await waitForSendBinding()
        if lock.withLock({ closed }) {
            throw ChannelError.closed
        }
        try await credit.consume()
        var buf = ByteBufferAllocator().buffer(capacity: 64)
        serialize(value, &buf)
        let bytes = buf.readBytes(length: buf.readableBytes) ?? []
        taskTx(.data(channelId: channelId, payload: bytes))
    }

    /// Close this channel.
    public func close() {
        let shouldClose = lock.withLock {
            if closed {
                return false
            }
            closed = true
            return true
        }
        guard shouldClose else {
            return
        }
        if let credit {
            Task {
                await credit.close()
            }
        }
        taskTx?(.close(channelId: channelId))
    }

    func finishRetryBinding() {
        close()
        (pairedRx as? AnyRetryFinalizableChannel)?.finishRetryBinding()
    }

    private func waitForSendBinding() async throws
        -> (@Sendable (TaskMessage) -> Void, ChannelCreditController)
    {
        while true {
            let state = lock.withLock {
                () -> (
                    taskTx: (@Sendable (TaskMessage) -> Void)?,
                    credit: ChannelCreditController?,
                    bound: Bool,
                    closed: Bool
                ) in
                (taskTx, credit, bound, closed)
            }

            if state.closed {
                throw ChannelError.closed
            }
            if let taskTx = state.taskTx, let credit = state.credit {
                return (taskTx, credit)
            }
            if state.bound {
                throw ChannelError.notBound
            }

            await withCheckedContinuation { continuation in
                let shouldResumeImmediately = lock.withLock { () -> Bool in
                    if closed || bound || (taskTx != nil && credit != nil) {
                        return true
                    }
                    bindingWaiters.append(continuation)
                    return false
                }
                if shouldResumeImmediately {
                    continuation.resume()
                }
            }
        }
    }
}

/// Unbound Rx - created by `channel()`, bound at call time.
public final class UnboundRx<T: Sendable>: @unchecked Sendable {
    public private(set) var channelId: ChannelId = 0
    private let deserialize: @Sendable (inout ByteBuffer) throws -> T
    private var bound = false
    private let lock = NSLock()
    private var bindingWaiters: [CheckedContinuation<Void, Never>] = []
    private var receivers: [ChannelReceiver] = []
    private var retryFinalized = false

    // Weak reference to paired Tx
    weak var pairedTx: AnyObject?

    public init(deserialize: @escaping @Sendable (inout ByteBuffer) throws -> T) {
        self.deserialize = deserialize
    }

    public var isBound: Bool { bound }

    /// Bind for receiving (client-side incoming).
    func bind(channelId: ChannelId, receiver: ChannelReceiver) {
        let waiters = lock.withLock { () -> [CheckedContinuation<Void, Never>] in
            self.channelId = channelId
            self.bound = true
            self.receivers.append(receiver)
            let waiters = self.bindingWaiters
            self.bindingWaiters.removeAll()
            return waiters
        }
        for waiter in waiters {
            waiter.resume()
        }
    }

    /// Set channel ID only (when paired Tx is bound).
    func setChannelIdOnly(channelId: ChannelId) {
        let waiters = lock.withLock { () -> [CheckedContinuation<Void, Never>] in
            self.channelId = channelId
            self.bound = true
            let waiters = self.bindingWaiters
            self.bindingWaiters.removeAll()
            return waiters
        }
        for waiter in waiters {
            waiter.resume()
        }
    }

    /// Receive the next value, or nil if closed.
    public func recv() async throws -> T? {
        while true {
            let receiver = lock.withLock { receivers.first }
            if let receiver {
                if let bytes = await receiver.recv() {
                    var buf = ByteBufferAllocator().buffer(capacity: bytes.count)
                    buf.writeBytes(bytes)
                    return try deserialize(&buf)
                }

                let shouldEnd = lock.withLock { () -> Bool in
                    if let head = receivers.first, head === receiver {
                        receivers.removeFirst()
                    }
                    return retryFinalized && receivers.isEmpty
                }
                if shouldEnd {
                    return nil
                }
                continue
            }

            let shouldEnd = lock.withLock { retryFinalized && receivers.isEmpty }
            if shouldEnd {
                return nil
            }
            await withCheckedContinuation { continuation in
                let shouldResumeImmediately = lock.withLock { () -> Bool in
                    if !receivers.isEmpty || (retryFinalized && receivers.isEmpty) {
                        return true
                    }
                    bindingWaiters.append(continuation)
                    return false
                }
                if shouldResumeImmediately {
                    continuation.resume()
                }
            }
        }
    }

    func finishRetryBinding() {
        let waiters = lock.withLock { () -> [CheckedContinuation<Void, Never>] in
            retryFinalized = true
            let waiters = bindingWaiters
            bindingWaiters.removeAll()
            return waiters
        }
        for waiter in waiters {
            waiter.resume()
        }
    }
}

// MARK: - AsyncSequence for UnboundRx

extension UnboundRx: AsyncSequence {
    public typealias Element = T

    public func makeAsyncIterator() -> AsyncIterator {
        AsyncIterator(rx: self)
    }

    public struct AsyncIterator: AsyncIteratorProtocol {
        let rx: UnboundRx<T>

        public mutating func next() async throws -> T? {
            try await rx.recv()
        }
    }
}

// MARK: - Channel Factory

/// Create paired unbound channels.
public func channel<T: Sendable>(
    serialize: @escaping @Sendable (T, inout ByteBuffer) -> Void,
    deserialize: @escaping @Sendable (inout ByteBuffer) throws -> T
) -> (UnboundTx<T>, UnboundRx<T>) {
    let tx = UnboundTx<T>(serialize: serialize)
    let rx = UnboundRx<T>(deserialize: deserialize)
    tx.pairedRx = rx
    rx.pairedTx = tx
    return (tx, rx)
}

// MARK: - Task Sender

/// Type alias for task message sender.
public typealias TaskSender = @Sendable (TaskMessage) -> Void

// MARK: - Incoming Channel Registry

/// Type alias for incoming channel registry.
public typealias IncomingChannelRegistry = ChannelRegistry

// Channel binding by schema-walking (TypeRef/Schema/SchemaKind) was removed:
// channels are now bound OUT-OF-BAND via the generated code's PhonChannelMeta
// (arg index + direction + element root), not by resolving the args schema here.

// MARK: - Type Erasure for Binding

/// Protocol for type-erased UnboundRx binding.
protocol AnyUnboundRx: AnyObject {
    func bindForSchema(
        channelId: ChannelId,
        taskSender: @escaping TaskSender,
        credit: ChannelCreditController
    )
    func channelIdForSchema() -> ChannelId
}

/// Protocol for type-erased UnboundTx binding.
protocol AnyUnboundTx: AnyObject {
    func bindForSchema(channelId: ChannelId, receiver: ChannelReceiver)
    func channelIdForSchema() -> ChannelId
}

extension UnboundRx: AnyUnboundRx {
    func bindForSchema(
        channelId: ChannelId,
        taskSender: @escaping TaskSender,
        credit: ChannelCreditController
    ) {
        // Schema Rx = client sends via Tx, so bind the paired Tx
        if let pairedTx = self.pairedTx as? AnyUnboundTxSender {
            pairedTx.bindForSending(channelId: channelId, taskSender: taskSender, credit: credit)
        }
        self.setChannelIdOnly(channelId: channelId)
    }

    func channelIdForSchema() -> ChannelId {
        channelId
    }
}

extension UnboundTx: AnyUnboundTx {
    func bindForSchema(channelId: ChannelId, receiver: ChannelReceiver) {
        // Schema Tx = client receives via Rx, so this Tx just gets ID
        self.setChannelIdOnly(channelId: channelId)
        if let pairedRx = self.pairedRx as? AnyUnboundRxReceiver {
            pairedRx.bindForReceiving(channelId: channelId, receiver: receiver)
        }
    }

    func channelIdForSchema() -> ChannelId {
        channelId
    }
}

/// Protocol for sending via Tx.
protocol AnyUnboundTxSender: AnyObject {
    func bindForSending(
        channelId: ChannelId,
        taskSender: @escaping TaskSender,
        credit: ChannelCreditController
    )
}

protocol AnyRetryFinalizableChannel: AnyObject {
    func finishRetryBinding()
}

extension UnboundTx: AnyUnboundTxSender {
    func bindForSending(
        channelId: ChannelId,
        taskSender: @escaping TaskSender,
        credit: ChannelCreditController
    ) {
        self.bind(channelId: channelId, taskTx: taskSender, credit: credit)
    }
}

extension UnboundTx: AnyRetryFinalizableChannel {}

protocol AnyUnboundRxReceiver: AnyObject {
    func bindForReceiving(channelId: ChannelId, receiver: ChannelReceiver)
}

extension UnboundRx: AnyUnboundRxReceiver {
    func bindForReceiving(channelId: ChannelId, receiver: ChannelReceiver) {
        self.bind(channelId: channelId, receiver: receiver)
    }
}

extension UnboundRx: AnyRetryFinalizableChannel {}
