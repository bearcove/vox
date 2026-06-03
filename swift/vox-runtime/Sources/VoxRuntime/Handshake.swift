import Foundation
import PhonEngine
import PhonSchema

// The handshake messages (HandshakeMessage / Hello / HelloYourself / LetsGo / Sorry)
// are generated in HandshakeWire.swift. Each is sent as one Link frame in phon
// self-describing framing: [u32 schema_len LE][closure][value] — see Messages.swift
// (encodeHandshakeFrame / decodeHandshakeFrame).

func sendHandshake(_ link: any Link, _ message: HandshakeMessage) async throws {
    traceLog(.handshake, "send")
    try await link.sendRawPrologue(encodeHandshakeFrame(message))
}

func recvHandshake(_ link: any Link) async throws -> HandshakeMessage {
    traceLog(.handshake, "recv waiting")
    guard let bytes = try await link.recvRawPrologue() else {
        throw ConnectionError.connectionClosed
    }
    let message = try decodeHandshakeFrame(bytes)
    traceLog(.handshake, "recv got message")
    return message
}

/// Whether the peer's advertised Message schema closure is usable. phon ids are
/// content-addressed and the conduit builds writer→reader compatibility decode, so any parseable
/// closure is accepted (a true incompatibility surfaces later as a decode error).
func handshakeMessageSchemasMatch(_ peerSchema: [UInt8]) -> Bool {
    (try? parseSchemaClosure(peerSchema)) != nil
}
