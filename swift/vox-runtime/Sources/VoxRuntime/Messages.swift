import Foundation
import PhonEngine
import PhonIR
import PhonSchema

// Message envelope constructors + self-describing framing, mirroring the TypeScript
// `vox-wire/src/types.ts` (`messageRequest`, `messageData`, …) and `codec.ts`
// (`buildMessageDecoder`, handshake framing). The generated `Wire.swift` /
// `HandshakeWire.swift` carry the types + `encodeMessage`/`decodeMessage`; this file
// is the hand-written glue.

// MARK: - Message constructors

func messageRequest(
    requestId: UInt64,
    methodId: UInt64,
    payload: [UInt8],
    metadata: Metadata = .null,
    channels: [UInt64] = [],
    connectionId: UInt64 = 0,
    schemas: [UInt8] = []
) -> Message {
    Message(
        connectionId: connectionId,
        payload: .requestMessage(RequestMessage(
            id: requestId,
            body: .call(RequestCall(
                methodId: methodId,
                channels: channels,
                metadata: metadata,
                args: Data(payload),
                schemas: Data(schemas))))))
}

func messageResponse(
    requestId: UInt64,
    payload: [UInt8],
    metadata: Metadata = .null,
    connectionId: UInt64 = 0,
    schemas: [UInt8] = []
) -> Message {
    Message(
        connectionId: connectionId,
        payload: .requestMessage(RequestMessage(
            id: requestId,
            body: .response(RequestResponse(
                metadata: metadata,
                ret: Data(payload),
                schemas: Data(schemas))))))
}

func messageCancel(
    requestId: UInt64,
    metadata: Metadata = .null,
    connectionId: UInt64 = 0
) -> Message {
    Message(
        connectionId: connectionId,
        payload: .requestMessage(RequestMessage(
            id: requestId,
            body: .cancel(RequestCancel(metadata: metadata)))))
}

func messageConnect(
    connectionId: UInt64,
    settings: ConnectionSettings,
    metadata: Metadata = .null
) -> Message {
    Message(
        connectionId: connectionId,
        payload: .connectionOpen(ConnectionOpen(connectionSettings: settings, metadata: metadata)))
}

func messageAccept(
    connectionId: UInt64,
    settings: ConnectionSettings,
    metadata: Metadata = .null
) -> Message {
    Message(
        connectionId: connectionId,
        payload: .connectionAccept(ConnectionAccept(connectionSettings: settings, metadata: metadata)))
}

func messageReject(connectionId: UInt64, metadata: Metadata = .null) -> Message {
    Message(connectionId: connectionId, payload: .connectionReject(ConnectionReject(metadata: metadata)))
}

func messageConnectionClose(connectionId: UInt64, metadata: Metadata = .null) -> Message {
    Message(connectionId: connectionId, payload: .connectionClose(ConnectionClose(metadata: metadata)))
}

func messageData(channelId: UInt64, item: [UInt8], connectionId: UInt64 = 0) -> Message {
    Message(
        connectionId: connectionId,
        payload: .channelMessage(ChannelMessage(id: channelId, body: .item(ChannelItem(item: Data(item))))))
}

func messageChannelClose(channelId: UInt64, connectionId: UInt64 = 0, metadata: Metadata = .null) -> Message {
    Message(
        connectionId: connectionId,
        payload: .channelMessage(ChannelMessage(id: channelId, body: .close(ChannelClose(metadata: metadata)))))
}

func messageChannelReset(channelId: UInt64, connectionId: UInt64 = 0, metadata: Metadata = .null) -> Message {
    Message(
        connectionId: connectionId,
        payload: .channelMessage(ChannelMessage(id: channelId, body: .reset(ChannelReset(metadata: metadata)))))
}

func messageCredit(channelId: UInt64, additional: UInt32, connectionId: UInt64 = 0) -> Message {
    Message(
        connectionId: connectionId,
        payload: .channelMessage(ChannelMessage(id: channelId, body: .grantCredit(ChannelGrantCredit(additional: additional)))))
}

func messageProtocolError(description: String, connectionId: UInt64 = 0) -> Message {
    Message(connectionId: connectionId, payload: .protocolError(ProtocolError(description: description)))
}

func messagePing(nonce: UInt64, connectionId: UInt64 = 0) -> Message {
    Message(connectionId: connectionId, payload: .ping(Ping(nonce: nonce)))
}

func messagePong(nonce: UInt64, connectionId: UInt64 = 0) -> Message {
    Message(connectionId: connectionId, payload: .pong(Pong(nonce: nonce)))
}

// MARK: - Message decoder (writer ⋈ reader)

/// A decoder that reconciles a peer's advertised (writer) Message schema against the
/// local reader. When the peer advertises nothing — or the same content-addressed
/// root — this is the degenerate same-schema decode (ids match cross-language).
/// Not `@Sendable`: it may capture a (non-Sendable) `MemProgram`; the conduit that
/// holds it is `@unchecked Sendable` and only invokes it from its own recv loop.
public typealias MessageDecoder = ([UInt8]) throws -> Message

/// Build the Message decoder for a peer, given the peer's `message_payload_schema`
/// closure (from the handshake). Mirrors TS `buildMessageDecoder`.
public func buildMessageDecoder(peerMessageSchema: [UInt8]?) -> MessageDecoder {
    guard let peer = peerMessageSchema, !peer.isEmpty,
        let bundle = try? parseSchemaClosure(peer),
        bundle.root != MessageRootId
    else {
        // No peer schema, or identical root: the generated same-schema decode (itself
        // the degenerate `lowerDecode(local → local)`).
        return { try decodeMessage($0) }
    }
    // Version skew: reconcile the peer's writer root against the local Message reader.
    let reg = MessageRegistry.with(bundle.schemas)
    guard let program = try? lowerDecode(bundle.root, MessageDescriptor, reg) else {
        return { try decodeMessage($0) }
    }
    return { bytes in try decodeWith(program, bytes, as: Message.self) }
}

/// Decode `bytes` into `T` through a pre-lowered decode program.
func decodeWith<T>(_ program: MemProgram, _ bytes: [UInt8], as _: T.Type) throws -> T {
    let raw = UnsafeMutableRawPointer.allocate(
        byteCount: MemoryLayout<T>.size, alignment: MemoryLayout<T>.alignment)
    defer { raw.deallocate() }
    try decodeInto(program, bytes, raw)
    return raw.assumingMemoryBound(to: T.self).move()
}

// MARK: - Handshake self-describing framing
//
// Each handshake message is one Link frame:
//   [u32 schema_len LE][schema-closure bytes][phon-compact value]

func encodeHandshakeFrame(_ msg: HandshakeMessage) -> [UInt8] {
    let value = encodeHandshakeMessage(msg)
    let closure = HandshakeMessageSchemaClosure
    var out = [UInt8]()
    out.reserveCapacity(4 + closure.count + value.count)
    let len = UInt32(closure.count).littleEndian
    withUnsafeBytes(of: len) { out.append(contentsOf: $0) }
    out.append(contentsOf: closure)
    out.append(contentsOf: value)
    return out
}

func decodeHandshakeFrame(_ bytes: [UInt8]) throws -> HandshakeMessage {
    guard bytes.count >= 4 else { throw ConnectionError.handshakeFailed("handshake frame too short") }
    let len = Int(bytes[0]) | (Int(bytes[1]) << 8) | (Int(bytes[2]) << 16) | (Int(bytes[3]) << 24)
    guard bytes.count >= 4 + len else { throw ConnectionError.handshakeFailed("handshake frame truncated") }
    let closure = Array(bytes[4..<(4 + len)])
    let value = Array(bytes[(4 + len)...])
    guard let bundle = try? parseSchemaClosure(closure), bundle.root != HandshakeMessageRootId else {
        // Same-schema (or unparseable): degenerate local decode.
        return try decodeHandshakeMessage(value)
    }
    let reg = HandshakeMessageRegistry.with(bundle.schemas)
    let program = try lowerDecode(bundle.root, HandshakeMessageDescriptor, reg)
    return try decodeWith(program, value, as: HandshakeMessage.self)
}
