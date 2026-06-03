import Foundation
@preconcurrency import NIOCore
import Testing

@testable import VoxRuntime

// Local little-endian i32 codec — this suite tests channel credit/flow-control, not the
// element codec, so it just needs to put/read i32 values in channel payloads.
private enum TestCodecError: Error { case shortRead }
private func encI32(_ v: Int32, into buf: inout ByteBuffer) {
    buf.writeInteger(v, endianness: .little)
}
private func decI32(from buf: inout ByteBuffer) throws -> Int32 {
    guard let v = buf.readInteger(endianness: .little, as: Int32.self) else {
        throw TestCodecError.shortRead
    }
    return v
}

private final class GrantInbox: @unchecked Sendable {
    private let lock = NSLock()
    private var grants: [UInt32] = []

    func append(_ value: UInt32) {
        lock.lock()
        grants.append(value)
        lock.unlock()
    }

    func snapshot() -> [UInt32] {
        lock.lock()
        defer { lock.unlock() }
        return grants
    }
}

private final class PayloadInbox: @unchecked Sendable {
    private let lock = NSLock()
    private var payloads: [[UInt8]] = []

    func append(_ payload: [UInt8]) {
        lock.lock()
        payloads.append(payload)
        lock.unlock()
    }

    func snapshot() -> [[UInt8]] {
        lock.lock()
        defer { lock.unlock() }
        return payloads
    }
}

@Suite(.serialized)
struct ChannelFlowControlTests {
    // r[verify schema.interaction.channels]
    @Test func senderWaitsForGrantCredit() async throws {
        let registry = ChannelRegistry()
        let payloads = PayloadInbox()
        let credit = await registry.registerOutgoing(1, initialCredit: 1)
        let tx = Tx<Int32>(serialize: { val, buf in encI32(val, into: &buf) })
        tx.bind(
            channelId: 1,
            taskTx: { message in
                guard case .data(_, let payload) = message else {
                    return
                }
                payloads.append(payload)
            }, credit: credit)

        try await tx.send(1)

        let secondSend = Task {
            try await tx.send(2)
        }

        for _ in 0..<10 {
            await Task.yield()
        }

        let beforeGrant = payloads.snapshot()
        #expect(beforeGrant.count == 1)
        var beforeBuf = ByteBufferAllocator().buffer(bytes: beforeGrant[0])
        #expect(try decI32(from: &beforeBuf) == 1)

        await registry.deliverCredit(channelId: 1, bytes: 1)
        try await secondSend.value

        let afterGrant = payloads.snapshot()
        #expect(afterGrant.count == 2)
        var afterBuf = ByteBufferAllocator().buffer(bytes: afterGrant[1])
        #expect(try decI32(from: &afterBuf) == 2)
    }

    @Test func receiverBatchesGrantCreditAtHalfWindow() async throws {
        let registry = ChannelRegistry()
        let inbox = GrantInbox()
        let receiver = await registry.register(7, initialCredit: 16) { additional in
            inbox.append(additional)
        }

        for i in 0..<8 {
            var buf = ByteBufferAllocator().buffer(capacity: 4)
            encI32(Int32(i), into: &buf)
            receiver.deliver(buf.readBytes(length: buf.readableBytes) ?? [])
        }

        for i in 0..<8 {
            let bytes = await receiver.recv()
            #expect(bytes != nil)
            var itemBuf = ByteBufferAllocator().buffer(bytes: bytes!)
            let value = try decI32(from: &itemBuf)
            #expect(value == Int32(i))
        }

        let grants = inbox.snapshot()
        #expect(grants == [8])
    }
}
