@preconcurrency import NIO
@preconcurrency import NIOCore
@preconcurrency import NIOPosix

private let defaultMaxFrameBytes = 1024 * 1024

private enum QueuedFrame {
    case frame([UInt8])
    case failure(Error)
    case eof

    func value() throws -> [UInt8]? {
        switch self {
        case .frame(let bytes):
            return bytes
        case .failure(let error):
            throw error
        case .eof:
            return nil
        }
    }

    func resume(_ continuation: CheckedContinuation<[UInt8]?, Error>) {
        switch self {
        case .frame(let bytes):
            continuation.resume(returning: bytes)
        case .failure(let error):
            continuation.resume(throwing: error)
        case .eof:
            continuation.resume(returning: nil)
        }
    }
}

private actor CancelSafeFrameQueue {
    private var pending: [QueuedFrame] = []
    private var waiters: [(UInt64, CheckedContinuation<[UInt8]?, Error>)] = []
    private var cancelledWaiters = Set<UInt64>()
    private var nextWaiterId: UInt64 = 0
    private var finished = false

    func push(_ result: Result<[UInt8], Error>) {
        switch result {
        case .success(let bytes):
            deliver(.frame(bytes))
        case .failure(let error):
            deliver(.failure(error))
        }
    }

    func finish() {
        finished = true
        while !waiters.isEmpty {
            let (_, continuation) = waiters.removeFirst()
            continuation.resume(returning: nil)
        }
    }

    func recv() async throws -> [UInt8]? {
        if !pending.isEmpty {
            return try pending.removeFirst().value()
        }
        if finished {
            return nil
        }

        let id = nextWaiterId
        nextWaiterId += 1

        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                if cancelledWaiters.remove(id) != nil || Task.isCancelled {
                    continuation.resume(throwing: CancellationError())
                } else if !pending.isEmpty {
                    pending.removeFirst().resume(continuation)
                } else if finished {
                    continuation.resume(returning: nil)
                } else {
                    waiters.append((id, continuation))
                }
            }
        } onCancel: {
            Task {
                await self.cancelWaiter(id)
            }
        }
    }

    private func deliver(_ item: QueuedFrame) {
        if waiters.isEmpty {
            pending.append(item)
        } else {
            let (_, continuation) = waiters.removeFirst()
            item.resume(continuation)
        }
    }

    private func cancelWaiter(_ id: UInt64) {
        if let index = waiters.firstIndex(where: { $0.0 == id }) {
            let (_, continuation) = waiters.remove(at: index)
            continuation.resume(throwing: CancellationError())
        } else {
            cancelledWaiters.insert(id)
        }
    }
}

// r[impl transport.stream]
// r[impl transport.stream.kinds]
// r[impl link]
// r[impl link.message]
// r[impl link.order]
public final class NIOFrameLink: Link, @unchecked Sendable {
    private let channel: Channel
    private let frameLimit: FrameLimit
    private let frameQueue: CancelSafeFrameQueue
    private let inboundPump: Task<Void, Never>
    private var owningGroup: MultiThreadedEventLoopGroup?

    init(
        channel: Channel,
        frameLimit: FrameLimit,
        inboundStream: AsyncStream<Result<[UInt8], Error>>,
        owningGroup: MultiThreadedEventLoopGroup? = nil
    ) {
        self.channel = channel
        self.frameLimit = frameLimit
        let frameQueue = CancelSafeFrameQueue()
        self.frameQueue = frameQueue
        self.inboundPump = Task {
            var iterator = inboundStream.makeAsyncIterator()
            while let result = await iterator.next() {
                await frameQueue.push(result)
            }
            await frameQueue.finish()
        }
        self.owningGroup = owningGroup
    }

    // r[impl link.tx.cancel-safe]
    public func sendFrame(_ bytes: [UInt8]) async throws {
        let frameLimit = self.frameLimit
        let maxFrameBytes = try await channel.eventLoop.submit {
            frameLimit.maxFrameBytes
        }.get()
        guard bytes.count <= maxFrameBytes else {
            throw TransportError.frameEncoding("Frame exceeds \(maxFrameBytes) bytes")
        }
        try await writeRawFrame(channel: channel, bytes: bytes)
    }

    // r[impl link.rx.recv]
    // r[impl link.rx.eof]
    // r[impl link.rx.error]
    // r[impl rpc.transport.stream.cancel-safe-recv]
    public func recvFrame() async throws -> [UInt8]? {
        try await frameQueue.recv()
    }

    // r[impl link.tx.alloc.limits]
    public func setMaxFrameSize(_ size: Int) async throws {
        let frameLimit = self.frameLimit
        try await channel.eventLoop.submit {
            frameLimit.maxFrameBytes = size
        }.get()
    }

    // r[impl link.tx.close]
    public func close() async throws {
        if channel.isActive {
            try? await channel.close()
        }
        inboundPump.cancel()
        await frameQueue.finish()
        if let group = owningGroup {
            owningGroup = nil
            try await group.shutdownGracefully()
        }
    }

    func socketKeepaliveEnabled() async throws -> Bool {
        let value = try await channel.getOption(ChannelOptions.socketOption(.so_keepalive)).get()
        return value != 0
    }
}

// r[impl transport.stream]
// r[impl rpc.transport.stream.cancel-safe-recv]
// r[impl link.message.empty]
final class LengthPrefixDecoder: ByteToMessageDecoder, @unchecked Sendable {
    typealias InboundOut = [UInt8]
    private let frameLimit: FrameLimit

    init(frameLimit: FrameLimit) {
        self.frameLimit = frameLimit
    }

    func decode(context: ChannelHandlerContext, buffer: inout ByteBuffer) throws -> DecodingState {
        guard let frameLen: UInt32 = buffer.getInteger(at: buffer.readerIndex, endianness: .little)
        else {
            return .needMoreData
        }

        let frameLength = Int(frameLen)
        let maxFrameBytes = frameLimit.maxFrameBytes
        if frameLength > maxFrameBytes {
            throw TransportError.frameDecoding("Frame exceeds \(maxFrameBytes) bytes")
        }

        let needed = 4 + frameLength
        guard buffer.readableBytes >= needed else {
            return .needMoreData
        }

        buffer.moveReaderIndex(forwardBy: 4)

        guard let frameBytes = buffer.readBytes(length: frameLength) else {
            return .needMoreData
        }

        context.fireChannelRead(wrapInboundOut(frameBytes))
        return .continue
    }

    func decodeLast(
        context: ChannelHandlerContext,
        buffer: inout ByteBuffer,
        seenEOF: Bool
    ) throws -> DecodingState {
        let state = try decode(context: context, buffer: &buffer)
        if state == .needMoreData && seenEOF && buffer.readableBytes > 0 {
            throw TransportError.frameDecoding(
                "EOF with \(buffer.readableBytes) trailing bytes and no complete frame"
            )
        }
        return state
    }
}

final class RawFrameStreamHandler: ChannelInboundHandler, RemovableChannelHandler, @unchecked Sendable {
    typealias InboundIn = [UInt8]

    private let continuation: AsyncStream<Result<[UInt8], Error>>.Continuation

    init(continuation: AsyncStream<Result<[UInt8], Error>>.Continuation) {
        self.continuation = continuation
    }

    func channelRead(context: ChannelHandlerContext, data: NIOAny) {
        continuation.yield(.success(unwrapInboundIn(data)))
    }

    // r[impl link.rx.error]
    func errorCaught(context: ChannelHandlerContext, error: Error) {
        continuation.yield(.failure(error))
        continuation.finish()
        context.close(promise: nil)
    }

    // r[impl link.rx.eof]
    func channelInactive(context: ChannelHandlerContext) {
        continuation.finish()
    }
}

// r[impl transport.stream]
// r[impl link.message.empty]
// r[impl link.tx.send]
// r[impl link.tx.cancel-safe]
func writeRawFrame(channel: Channel, bytes: [UInt8]) async throws {
    guard let len = UInt32(exactly: bytes.count) else {
        throw TransportError.frameEncoding("frame too large for u32 length prefix")
    }

    var buffer = channel.allocator.buffer(capacity: 4 + bytes.count)
    buffer.writeInteger(len, endianness: .little)
    buffer.writeBytes(bytes)
    try await channel.writeAndFlush(buffer)
}

// r[impl transport.stream.local]
// r[impl transport.stream.kinds]
func connectLink(unixPath: String) async throws -> NIOFrameLink {
    let frameLimit = FrameLimit(defaultMaxFrameBytes)
    let group = MultiThreadedEventLoopGroup(numberOfThreads: 1)

    var rawContinuation: AsyncStream<Result<[UInt8], Error>>.Continuation!
    let rawStream = AsyncStream<Result<[UInt8], Error>> { continuation in
        rawContinuation = continuation
    }
    let rawHandler = RawFrameStreamHandler(continuation: rawContinuation!)

    let bootstrap = ClientBootstrap(group: group)
        .channelInitializer { channel in
            do {
                try channel.pipeline.syncOperations.addHandler(
                    ByteToMessageHandler(LengthPrefixDecoder(frameLimit: frameLimit))
                )
                try channel.pipeline.syncOperations.addHandler(rawHandler)
                return channel.eventLoop.makeSucceededVoidFuture()
            } catch {
                return channel.eventLoop.makeFailedFuture(error)
            }
        }

    do {
        let channel = try await bootstrap.connect(unixDomainSocketPath: unixPath).get()
        return NIOFrameLink(
            channel: channel,
            frameLimit: frameLimit,
            inboundStream: rawStream,
            owningGroup: group
        )
    } catch {
        try? await group.shutdownGracefully()
        throw error
    }
}

// r[impl transport.stream]
// r[impl transport.stream.kinds]
func connectLink(host: String, port: Int) async throws -> NIOFrameLink {
    let frameLimit = FrameLimit(defaultMaxFrameBytes)
    let group = MultiThreadedEventLoopGroup(numberOfThreads: 1)

    var rawContinuation: AsyncStream<Result<[UInt8], Error>>.Continuation!
    let rawStream = AsyncStream<Result<[UInt8], Error>> { continuation in
        rawContinuation = continuation
    }
    let rawHandler = RawFrameStreamHandler(continuation: rawContinuation!)

    let bootstrap = ClientBootstrap(group: group)
        .channelOption(ChannelOptions.socketOption(.so_keepalive), value: 1)
        .channelInitializer { channel in
            do {
                try channel.pipeline.syncOperations.addHandler(
                    ByteToMessageHandler(LengthPrefixDecoder(frameLimit: frameLimit))
                )
                try channel.pipeline.syncOperations.addHandler(rawHandler)
                return channel.eventLoop.makeSucceededVoidFuture()
            } catch {
                return channel.eventLoop.makeFailedFuture(error)
            }
        }

    do {
        let channel = try await bootstrap.connect(host: host, port: port).get()
        return NIOFrameLink(
            channel: channel,
            frameLimit: frameLimit,
            inboundStream: rawStream,
            owningGroup: group
        )
    } catch {
        try? await group.shutdownGracefully()
        throw error
    }
}
