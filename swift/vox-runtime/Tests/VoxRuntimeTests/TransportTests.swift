import Foundation
@preconcurrency import NIO
@preconcurrency import NIOPosix
import Testing

@testable import VoxRuntime

private struct LocalServer {
    let group: MultiThreadedEventLoopGroup
    let channel: Channel
    let port: Int
}

private struct LocalUnixServer {
    let group: MultiThreadedEventLoopGroup
    let channel: Channel
    let path: String
}

private let transportAcceptBareBytes: [UInt8] = Array("VOTA".utf8) + [9, 0, 0, 0]

private actor FrameCapture {
    private var frames: [[UInt8]] = []
    private var inactive = false

    func record(_ bytes: [UInt8]) {
        frames.append(bytes)
    }

    func markInactive() {
        inactive = true
    }

    func waitForFrameCount(_ count: Int, timeoutMs: UInt64 = 1_000) async -> [[UInt8]]? {
        let start = ContinuousClock.now
        let timeout = Duration.milliseconds(Int64(timeoutMs))
        while ContinuousClock.now - start < timeout {
            if frames.count >= count {
                return frames
            }
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
        return nil
    }

    func waitForInactive(timeoutMs: UInt64 = 1_000) async -> Bool {
        let start = ContinuousClock.now
        let timeout = Duration.milliseconds(Int64(timeoutMs))
        while ContinuousClock.now - start < timeout {
            if inactive {
                return true
            }
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
        return inactive
    }
}

private func writeLengthPrefixedFrame(_ bytes: [UInt8], to channel: Channel) {
    var buffer = channel.allocator.buffer(capacity: 4 + bytes.count)
    buffer.writeInteger(UInt32(bytes.count), endianness: .little)
    buffer.writeBytes(bytes)
    channel.write(buffer, promise: nil)
}

private final class WriteOnActiveHandler: ChannelInboundHandler, Sendable {
    typealias InboundIn = Never

    private let bytes: [UInt8]

    init(bytes: [UInt8]) {
        self.bytes = bytes
    }

    func channelActive(context: ChannelHandlerContext) {
        var buffer = context.channel.allocator.buffer(capacity: 4 + bytes.count)
        buffer.writeInteger(UInt32(bytes.count), endianness: .little)
        buffer.writeBytes(bytes)
        context.writeAndFlush(NIOAny(buffer), promise: nil)
        context.fireChannelActive()
    }
}

private final class WriteFramesThenCloseHandler: ChannelInboundHandler, Sendable {
    typealias InboundIn = Never

    private let frames: [[UInt8]]

    init(frames: [[UInt8]]) {
        self.frames = frames
    }

    func channelActive(context: ChannelHandlerContext) {
        for frame in frames {
            writeLengthPrefixedFrame(frame, to: context.channel)
        }
        context.flush()
        context.close(promise: nil)
        context.fireChannelActive()
    }
}

private final class WriteRawThenCloseHandler: ChannelInboundHandler, Sendable {
    typealias InboundIn = Never

    private let bytes: [UInt8]

    init(bytes: [UInt8]) {
        self.bytes = bytes
    }

    func channelActive(context: ChannelHandlerContext) {
        var buffer = context.channel.allocator.buffer(capacity: bytes.count)
        buffer.writeBytes(bytes)
        context.writeAndFlush(NIOAny(buffer), promise: nil)
        context.close(promise: nil)
        context.fireChannelActive()
    }
}

private final class CaptureFramesHandler: ChannelInboundHandler, @unchecked Sendable {
    typealias InboundIn = [UInt8]

    private let capture: FrameCapture

    init(capture: FrameCapture) {
        self.capture = capture
    }

    func channelRead(context _: ChannelHandlerContext, data: NIOAny) {
        let frame = unwrapInboundIn(data)
        Task {
            await capture.record(frame)
        }
    }

    func errorCaught(context: ChannelHandlerContext, error _: Error) {
        context.close(promise: nil)
    }

    func channelInactive(context: ChannelHandlerContext) {
        Task {
            await capture.markInactive()
        }
        context.fireChannelInactive()
    }
}

private func startLocalServer(
    childChannelInitializer: @escaping @Sendable (Channel) -> EventLoopFuture<Void> = { channel in
        channel.pipeline.addHandler(WriteOnActiveHandler(bytes: transportAcceptBareBytes))
    }
) async throws -> LocalServer {
    let group = MultiThreadedEventLoopGroup(numberOfThreads: 1)
    let bootstrap = ServerBootstrap(group: group)
        .serverChannelOption(ChannelOptions.backlog, value: 8)
        .serverChannelOption(ChannelOptions.socketOption(.so_reuseaddr), value: 1)
        .childChannelInitializer(childChannelInitializer)

    let channel: Channel
    do {
        channel = try await bootstrap.bind(host: "127.0.0.1", port: 0).get()
    } catch {
        try? await group.shutdownGracefully()
        throw error
    }
    guard let port = channel.localAddress?.port else {
        if channel.isActive {
            try await channel.close()
        }
        try await group.shutdownGracefully()
        throw TransportError.connectionClosed
    }
    return LocalServer(group: group, channel: channel, port: port)
}

private func startLocalUnixServer(
    childChannelInitializer: @escaping @Sendable (Channel) -> EventLoopFuture<Void>
) async throws -> LocalUnixServer {
    let group = MultiThreadedEventLoopGroup(numberOfThreads: 1)
    let path = "\(NSTemporaryDirectory())vox-runtime-\(UUID().uuidString).sock"
    unlink(path)
    let bootstrap = ServerBootstrap(group: group)
        .serverChannelOption(ChannelOptions.backlog, value: 8)
        .childChannelInitializer(childChannelInitializer)

    let channel: Channel
    do {
        channel = try await bootstrap.bind(unixDomainSocketPath: path).get()
    } catch {
        try? await group.shutdownGracefully()
        unlink(path)
        throw error
    }
    return LocalUnixServer(group: group, channel: channel, path: path)
}

private func stopLocalServer(_ server: LocalServer) async {
    if server.channel.isActive {
        try? await server.channel.close()
    }
    try? await server.group.shutdownGracefully()
}

private func stopLocalUnixServer(_ server: LocalUnixServer) async {
    if server.channel.isActive {
        try? await server.channel.close()
    }
    try? await server.group.shutdownGracefully()
    unlink(server.path)
}

private actor TestLink: Link {
    func sendFrame(_: [UInt8]) async throws {}

    func recvFrame() async throws -> [UInt8]? {
        nil
    }

    func setMaxFrameSize(_: Int) async throws {}

    func close() async throws {}
}

@Suite(.serialized)
struct TransportTests {
    // r[verify link.split]
    @Test func singleLinkSourceYieldsOneFreshAttachment() async throws {
        let link = TestLink()
        let source = singleLinkSource(link)

        let first = try await source.nextLink()
        #expect(!first.hasCompletedPrologue)

        do {
            _ = try await source.nextLink()
            Issue.record("single link source yielded a second attachment")
        } catch let error as TransportError {
            guard case .protocolViolation(let message) = error else {
                Issue.record("unexpected error: \(error)")
                return
            }
            #expect(message == "single-use LinkSource exhausted")
        }
    }

    // r[verify link]
    // r[verify link.message]
    // r[verify link.message.empty]
    // r[verify link.order]
    // r[verify link.rx.recv]
    // r[verify link.rx.eof]
    // r[verify transport.stream]
    // r[verify transport.stream.kinds]
    @Test func tcpStreamLinkPreservesBoundariesOrderEmptyPayloadsAndEof() async throws {
        let server = try await startLocalServer { channel in
            channel.pipeline.addHandler(WriteFramesThenCloseHandler(frames: [[], [1], [2, 3]]))
        }
        do {
            let link = try await connectLink(host: "127.0.0.1", port: server.port)
            #expect(try await link.recvFrame() == [])
            #expect(try await link.recvFrame() == [1])
            #expect(try await link.recvFrame() == [2, 3])
            #expect(try await link.recvFrame() == nil)
            #expect(try await link.recvFrame() == nil)
            try? await link.close()
        } catch {
            await stopLocalServer(server)
            throw error
        }
        await stopLocalServer(server)
    }

    // r[verify link.tx.send]
    // r[verify link.tx.close]
    // r[verify link.tx.alloc.limits]
    @Test func tcpStreamLinkSendsFramesClosesAndRejectsOversizedPayloads() async throws {
        let capture = FrameCapture()
        let server = try await startLocalServer { channel in
            let frameLimit = FrameLimit(1024 * 1024)
            do {
                try channel.pipeline.syncOperations.addHandler(
                    ByteToMessageHandler(LengthPrefixDecoder(frameLimit: frameLimit))
                )
                try channel.pipeline.syncOperations.addHandler(CaptureFramesHandler(capture: capture))
                return channel.eventLoop.makeSucceededVoidFuture()
            } catch {
                return channel.eventLoop.makeFailedFuture(error)
            }
        }
        do {
            let link = try await connectLink(host: "127.0.0.1", port: server.port)
            try await link.setMaxFrameSize(2)
            do {
                try await link.sendFrame([1, 2, 3])
                Issue.record("oversized frame send unexpectedly succeeded")
            } catch let error as TransportError {
                guard case .frameEncoding(let message) = error else {
                    Issue.record("unexpected send error: \(error)")
                    await stopLocalServer(server)
                    return
                }
                #expect(message == "Frame exceeds 2 bytes")
            }

            try await link.sendFrame([4, 5])
            try await link.sendFrame([])
            guard let frames = await capture.waitForFrameCount(2) else {
                Issue.record("server did not observe committed frames")
                await stopLocalServer(server)
                return
            }
            #expect(frames == [[4, 5], []])
            try await link.close()
            #expect(await capture.waitForInactive())
        } catch {
            await stopLocalServer(server)
            throw error
        }
        await stopLocalServer(server)
    }

    // r[verify link.rx.error]
    @Test func tcpStreamLinkReceiveErrorIsTerminal() async throws {
        let server = try await startLocalServer { channel in
            channel.pipeline.addHandler(WriteRawThenCloseHandler(bytes: [3, 0, 0, 0, 1]))
        }
        do {
            let link = try await connectLink(host: "127.0.0.1", port: server.port)
            do {
                _ = try await link.recvFrame()
                Issue.record("partial frame unexpectedly decoded")
            } catch let error as TransportError {
                guard case .frameDecoding(let message) = error else {
                    Issue.record("unexpected receive error: \(error)")
                    await stopLocalServer(server)
                    return
                }
                #expect(message == "EOF with 5 trailing bytes and no complete frame")
            }
            #expect(try await link.recvFrame() == nil)
            try? await link.close()
        } catch {
            await stopLocalServer(server)
            throw error
        }
        await stopLocalServer(server)
    }

    // r[verify transport.stream.local]
    // r[verify transport.stream.kinds]
    @Test func unixStreamLinkConnectsToLocalSocketTransport() async throws {
        let server = try await startLocalUnixServer { channel in
            channel.pipeline.addHandler(WriteFramesThenCloseHandler(frames: [[7, 8, 9]]))
        }
        do {
            let link = try await connectLink(unixPath: server.path)
            #expect(try await link.recvFrame() == [7, 8, 9])
            #expect(try await link.recvFrame() == nil)
            try? await link.close()
        } catch {
            await stopLocalUnixServer(server)
            throw error
        }
        await stopLocalUnixServer(server)
    }

    // r[verify transport.prologue]
    // r[verify transport.prologue.request]
    // r[verify transport.prologue.accept]
    @Test func connectEnablesSocketKeepalive() async throws {
        let server = try await startLocalServer()
        do {
            let link = try await connectLink(host: "127.0.0.1", port: server.port)
            try await performInitiatorLinkPrologue(link: link)
            let keepalive = try await link.socketKeepaliveEnabled()
            #expect(keepalive)
            try? await link.close()
        } catch {
            await stopLocalServer(server)
            throw error
        }
        await stopLocalServer(server)
    }

    // r[verify transport.prologue.reject-close]
    @Test func transportPrologueRejectsUnsupportedPrologue() async throws {
        let server = try await startLocalServer { channel in
            channel.pipeline.addHandler(
                WriteOnActiveHandler(bytes: encodeTransportRejectUnsupported()))
        }
        do {
            let link = try await connectLink(host: "127.0.0.1", port: server.port)
            do {
                try await performInitiatorLinkPrologue(link: link)
                Issue.record("connect unexpectedly accepted rejected transport prologue")
            } catch let error as TransportError {
                guard case .protocolViolation(let message) = error else {
                    Issue.record("unexpected error: \(error)")
                    try? await link.close()
                    await stopLocalServer(server)
                    return
                }
                #expect(message == "transport rejected unsupported prologue")
            }
            try? await link.close()
        } catch {
            await stopLocalServer(server)
            throw error
        }
        await stopLocalServer(server)
    }

    @Test func transportPrologueTimesOutWhenServerNeverReplies() async throws {
        let server = try await startLocalServer { channel in
            channel.eventLoop.makeSucceededFuture(())
        }
        do {
            do {
                _ = try await connect(
                    host: "127.0.0.1",
                    port: server.port,
                    prologueTimeoutNs: 50_000_000
                )
                Issue.record("connect unexpectedly succeeded without transport prologue response")
            } catch let error as TransportError {
                guard case .protocolViolation(let message) = error else {
                    Issue.record("unexpected error: \(error)")
                    return
                }
                #expect(message == "transport prologue timed out")
            }
        }
        await stopLocalServer(server)
    }
}
