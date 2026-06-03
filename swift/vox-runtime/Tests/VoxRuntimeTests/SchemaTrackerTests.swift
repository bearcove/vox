import Foundation
import Testing
@preconcurrency import NIOCore
import PhonSchema

@testable import VoxRuntime

private func appendU32(_ value: UInt32, to bytes: inout [UInt8]) {
    let little = value.littleEndian
    withUnsafeBytes(of: little) { bytes.append(contentsOf: $0) }
}

private func appendU64(_ value: UInt64, to bytes: inout [UInt8]) {
    let little = value.littleEndian
    withUnsafeBytes(of: little) { bytes.append(contentsOf: $0) }
}

private func schemaClosure(root: UInt64, auxiliaryRoots: [(String, UInt64)] = []) -> [UInt8] {
    var bytes: [UInt8] = []
    appendU64(root, to: &bytes)
    appendU32(0, to: &bytes)
    if !auxiliaryRoots.isEmpty {
        appendU32(UInt32(auxiliaryRoots.count), to: &bytes)
        for (role, auxRoot) in auxiliaryRoots {
            let roleBytes = Array(role.utf8)
            appendU32(UInt32(roleBytes.count), to: &bytes)
            bytes.append(contentsOf: roleBytes)
            appendU64(auxRoot, to: &bytes)
        }
    }
    return bytes
}

private final class TaskInbox: @unchecked Sendable {
    private let lock = NSLock()
    private var messages: [TaskMessage] = []

    func append(_ message: TaskMessage) {
        lock.lock()
        messages.append(message)
        lock.unlock()
    }

    func snapshot() -> [TaskMessage] {
        lock.lock()
        defer { lock.unlock() }
        return messages
    }
}

@Test
// r[verify schema.format.delivery]
func schemaSendTrackerAdvertisesBindingOncePerDirection() {
    let tracker = SchemaSendTracker()
    let closure: [UInt8] = [1, 2, 3]

    #expect(tracker.prepareSchemas(7, .args, closure) == closure)
    #expect(tracker.prepareSchemas(7, .args, closure).isEmpty)
    #expect(tracker.prepareSchemas(7, .response, closure) == closure)
}

@Test
// r[verify schema.exchange.channels]
// r[verify schema.exchange.channels.rx-args]
func schemaTrackerRecordsChannelAuxiliaryRoots() {
    let tracker = SchemaTracker()
    let closure = schemaClosure(
        root: 1,
        auxiliaryRoots: [("channel.arg.0.rx.element", 2)]
    )

    tracker.recordReceived(7, .args, closure)

    #expect(tracker.auxiliaryRoot(7, .args, role: "channel.arg.0.rx.element") == SchemaId(2))
    #expect(tracker.auxiliaryRoot(7, .args, role: "channel.arg.1.rx.element") == nil)
}

@Test
// r[verify schema.exchange.channels.tx-args]
func serverTxAdvertisesArgsSchemaBeforeFirstData() async throws {
    let registry = ChannelRegistry()
    let inbox = TaskInbox()
    let tx = await bindServerTx(
        channelId: 9,
        registry: registry,
        taskTx: { inbox.append($0) },
        methodId: 77,
        argsSchemaClosure: [1, 2, 3],
        schemaSendTracker: SchemaSendTracker(),
        serialize: { (value: Int32, buf: inout ByteBuffer) in
            buf.writeInteger(value, endianness: .little)
        }
    )

    try await tx.send(42)

    let messages = inbox.snapshot()
    #expect(messages.count == 2)
    guard messages.count == 2 else { return }
    guard case .schema(let methodId, let direction, let schemas) = messages[0] else {
        Issue.record("first task message was not schema")
        return
    }
    #expect(methodId == 77)
    #expect(direction == .args)
    #expect(schemas == [1, 2, 3])
    guard case .data(let channelId, let payload) = messages[1] else {
        Issue.record("second task message was not data")
        return
    }
    #expect(channelId == 9)
    var buf = ByteBufferAllocator().buffer(bytes: payload)
    #expect(buf.readInteger(endianness: .little, as: Int32.self) == 42)
}
