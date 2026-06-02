import Foundation

struct SessionHandshakeResult {
    let negotiated: Negotiated
    let peerSupportsRetry: Bool
    let sessionResumeKey: [UInt8]?
    let localRootSettings: ConnectionSettings
    let peerRootSettings: ConnectionSettings
    let peerMetadata: Metadata
    /// The peer's advertised Message schema closure, used to build the conduit's
    /// reconciling decoder.
    let peerMessageSchema: [UInt8]
}

/// Generate a fresh 16-byte session resume key from the system CSPRNG.
/// Matches the Rust acceptor's `fresh_resume_key` (`[u8; 16]` via getrandom)
/// and the TypeScript acceptor's `randomSessionResumeKey` (16-byte
/// `crypto.getRandomValues`).
func freshResumeKey() -> [UInt8] {
    var rng = SystemRandomNumberGenerator()
    return (0..<16).map { _ in UInt8.random(in: UInt8.min...UInt8.max, using: &rng) }
}

func oppositeParity(_ parity: Parity) -> Parity {
    switch parity {
    case .odd:
        return .even
    case .even:
        return .odd
    }
}

func sendHandshakeSorry(_ link: any Link, reason: String) async {
    try? await sendHandshake(link, .sorry(Sorry(reason: reason)))
}

func requireIdentityMessageSchema(
    _ peerMessageSchema: [UInt8],
    on link: any Link
) async throws {
    guard handshakeMessageSchemasMatch(peerMessageSchema) else {
        let reason = "unsupported message schema translation"
        await sendHandshakeSorry(link, reason: reason)
        throw ConnectionError.handshakeFailed(reason)
    }
}

/// The local Message schema closure, advertised in the handshake.
private var localMessagePayloadSchema: [UInt8] { MessageSchemaClosure }

func performInitiatorHandshake(
    link: any Link,
    maxPayloadSize: UInt32,
    maxConcurrentRequests: UInt32,
    initialChannelCredit: UInt32 = 16,
    resumable: Bool,
    resumeKey: [UInt8]? = nil,
    metadata: Metadata = .null
) async throws -> SessionHandshakeResult {
    traceLog(.handshake, "initiator sending Hello resumable=\(resumable)")
    let ourSettings = ConnectionSettings(
        parity: .odd, maxConcurrentRequests: maxConcurrentRequests,
        initialChannelCredit: initialChannelCredit)
    let hello = Hello(
        parity: ourSettings.parity,
        connectionSettings: ourSettings,
        messagePayloadSchema: Data(localMessagePayloadSchema),
        resumeKey: resumeKey.map { ResumeKeyBytes(bytes: Data($0)) },
        metadata: appendRetrySupportMetadata(metadata)
    )
    try await sendHandshake(link, .hello(hello))

    let peerHello: HelloYourself
    switch try await recvHandshake(link) {
    case .helloYourself(let helloYourself):
        traceLog(.handshake, "initiator received HelloYourself")
        peerHello = helloYourself
    case .sorry(let sorry):
        throw ConnectionError.handshakeFailed(sorry.reason)
    default:
        await sendHandshakeSorry(link, reason: "expected HelloYourself or Sorry")
        throw ConnectionError.handshakeFailed("expected HelloYourself")
    }

    let peerSchema = [UInt8](peerHello.messagePayloadSchema)
    try await requireIdentityMessageSchema(peerSchema, on: link)

    let sessionResumeKey = peerHello.resumeKey.map { [UInt8]($0.bytes) }
    // If we requested resumable but the peer doesn't echo a resume key, that's
    // fine: we'll recover by establishing a fresh session and replaying in-flight
    // requests, rather than doing a true protocol-level resume.

    try await sendHandshake(link, .letsGo(LetsGo()))
    traceLog(.handshake, "initiator sent LetsGo")

    let negotiated = Negotiated(
        maxPayloadSize: maxPayloadSize,
        initialCredit: 64 * 1024,
        maxConcurrentRequests: min(
            ourSettings.maxConcurrentRequests,
            peerHello.connectionSettings.maxConcurrentRequests
        )
    )
    debugLog(
        "handshake complete: maxPayloadSize=\(negotiated.maxPayloadSize), "
            + "initialCredit=\(negotiated.initialCredit), "
            + "maxConcurrentRequests=\(negotiated.maxConcurrentRequests)"
    )

    return SessionHandshakeResult(
        negotiated: negotiated,
        peerSupportsRetry: metadataSupportsRetry(peerHello.metadata),
        sessionResumeKey: sessionResumeKey,
        localRootSettings: ourSettings,
        peerRootSettings: peerHello.connectionSettings,
        peerMetadata: peerHello.metadata,
        peerMessageSchema: peerSchema
    )
}

func performAcceptorHandshake(
    link: any Link,
    maxPayloadSize: UInt32,
    maxConcurrentRequests: UInt32,
    initialChannelCredit: UInt32 = 16,
    resumable: Bool,
    expectedResumeKey: [UInt8]? = nil,
    metadata: Metadata = .null
) async throws -> SessionHandshakeResult {
    let peerHello: Hello
    switch try await recvHandshake(link) {
    case .hello(let hello):
        traceLog(.handshake, "acceptor received Hello resumable=\(hello.resumeKey != nil)")
        peerHello = hello
    default:
        throw ConnectionError.handshakeFailed("expected Hello")
    }

    let peerSchema = [UInt8](peerHello.messagePayloadSchema)
    try await requireIdentityMessageSchema(peerSchema, on: link)

    // True protocol-level session resume / stable conduit was removed from
    // the Swift runtime, so the peer's resume key is not used to look up an
    // existing session.
    let _ = expectedResumeKey

    let ourSettings = ConnectionSettings(
        parity: oppositeParity(peerHello.parity),
        maxConcurrentRequests: maxConcurrentRequests,
        initialChannelCredit: initialChannelCredit
    )
    // Still advertise a fresh resume key when resumable, mirroring the Rust
    // and TypeScript acceptors. Reference initiators that request resumption
    // (the TS client rejects the handshake outright otherwise) require the
    // acceptor to echo a key; recovery is handled by replaying in-flight
    // requests on a fresh session rather than a true protocol-level resume.
    let sessionResumeKey: [UInt8]? = resumable ? freshResumeKey() : nil
    let helloYourself = HelloYourself(
        connectionSettings: ourSettings,
        messagePayloadSchema: Data(localMessagePayloadSchema),
        resumeKey: sessionResumeKey.map { ResumeKeyBytes(bytes: Data($0)) },
        metadata: appendRetrySupportMetadata(metadata)
    )
    try await sendHandshake(link, .helloYourself(helloYourself))
    traceLog(.handshake, "acceptor sent HelloYourself resumable=\(sessionResumeKey != nil)")

    switch try await recvHandshake(link) {
    case .letsGo:
        traceLog(.handshake, "acceptor received LetsGo")
        break
    case .sorry(let sorry):
        throw ConnectionError.handshakeFailed(sorry.reason)
    default:
        throw ConnectionError.handshakeFailed("expected LetsGo")
    }

    let negotiated = Negotiated(
        maxPayloadSize: maxPayloadSize,
        initialCredit: 64 * 1024,
        maxConcurrentRequests: min(
            ourSettings.maxConcurrentRequests,
            peerHello.connectionSettings.maxConcurrentRequests
        )
    )
    debugLog(
        "handshake complete: maxPayloadSize=\(negotiated.maxPayloadSize), "
            + "initialCredit=\(negotiated.initialCredit), "
            + "maxConcurrentRequests=\(negotiated.maxConcurrentRequests)"
    )

    return SessionHandshakeResult(
        negotiated: negotiated,
        peerSupportsRetry: metadataSupportsRetry(peerHello.metadata),
        sessionResumeKey: sessionResumeKey,
        localRootSettings: ourSettings,
        peerRootSettings: peerHello.connectionSettings,
        peerMetadata: peerHello.metadata,
        peerMessageSchema: peerSchema
    )
}

func buildEstablishedConduit(
    role: Role,
    transport: ConduitKind,
    attachment: LinkAttachment,
    peerMessageSchema: [UInt8],
    recoverAttachment: (@Sendable () async throws -> LinkAttachment)? = nil
) async throws -> any Conduit {
    let _ = role
    let _ = recoverAttachment
    // StableConduit was removed (had no real users); both kinds route to bare.
    return BareConduit(link: attachment.link, peerMessageSchema: peerMessageSchema)
}


func establishInitiator(
    attachment: LinkAttachment,
    transport: ConduitKind = .bare,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    resumable: Bool = false,
    recoverAttachment: (@Sendable () async throws -> LinkAttachment)? = nil,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, [UInt8]?, Metadata) {
    warnLog("[vox-establish] initiator: starting handshake")
    let ourMaxPayload = maxPayloadSize ?? (1024 * 1024)
    let handshake = try await performInitiatorHandshake(
        link: attachment.link,
        maxPayloadSize: ourMaxPayload,
        maxConcurrentRequests: 64,
        initialChannelCredit: initialChannelCredit,
        resumable: resumable,
        metadata: metadata
    )
    warnLog("[vox-establish] initiator: handshake done")

    let conduit = try await buildEstablishedConduit(
        role: .initiator,
        transport: transport,
        attachment: attachment,
        peerMessageSchema: handshake.peerMessageSchema,
        recoverAttachment: recoverAttachment
    )
    try await conduit.setMaxFrameSize(Int(handshake.negotiated.maxPayloadSize) + 64)

    let (connection, driver, handle) = makeSessionDriverAndConnection(
        conduit: conduit,
        dispatcher: dispatcher,
        role: .initiator,
        negotiated: handshake.negotiated,
        peerSupportsRetry: handshake.peerSupportsRetry,
        connectionAcceptor: connectionAcceptor,
        keepalive: keepalive,
        resumable: resumable,
        sessionResumeKey: handshake.sessionResumeKey,
        localRootSettings: handshake.localRootSettings,
        peerRootSettings: handshake.peerRootSettings,
        peerMessageSchema: handshake.peerMessageSchema,
        transport: transport,
        recoverAttachment: recoverAttachment
    )
    return (connection, driver, handle, handshake.sessionResumeKey, handshake.peerMetadata)
}

func establishInitiator(
    link: any Link,
    transport: ConduitKind = .bare,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    resumable: Bool = false,
    recoverAttachment: (@Sendable () async throws -> LinkAttachment)? = nil,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, [UInt8]?, Metadata) {
    try await establishInitiator(
        attachment: .initiator(link),
        transport: transport,
        dispatcher: dispatcher,
        connectionAcceptor: connectionAcceptor,
        maxPayloadSize: maxPayloadSize,
        initialChannelCredit: initialChannelCredit,
        keepalive: keepalive,
        resumable: resumable,
        recoverAttachment: recoverAttachment,
        metadata: metadata
    )
}

func establishInitiator(
    conduit: any Link,
    transport: ConduitKind = .bare,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    resumable: Bool = false,
    recoverAttachment: (@Sendable () async throws -> LinkAttachment)? = nil,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, [UInt8]?, Metadata) {
    try await establishInitiator(
        link: conduit,
        transport: transport,
        dispatcher: dispatcher,
        connectionAcceptor: connectionAcceptor,
        maxPayloadSize: maxPayloadSize,
        initialChannelCredit: initialChannelCredit,
        keepalive: keepalive,
        resumable: resumable,
        recoverAttachment: recoverAttachment,
        metadata: metadata
    )
}

func establishAcceptor(
    attachment: LinkAttachment,
    transport: ConduitKind = .bare,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    resumable: Bool = false,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, [UInt8]?, Metadata) {
    warnLog("[vox-establish] acceptor: negotiatedConduit=\(String(describing: attachment.negotiatedConduit)) transport=\(transport)")
    if attachment.negotiatedConduit == nil {
        warnLog("[vox-establish] acceptor: running link prologue")
        let negotiatedTransport = try await performAcceptorLinkPrologue(
            link: attachment.link,
            supportedConduit: transport
        )
        warnLog("[vox-establish] acceptor: prologue done, negotiated=\(negotiatedTransport)")
        guard negotiatedTransport == transport else {
            throw TransportError.protocolViolation(
                "transport negotiated \(negotiatedTransport) for requested \(transport)"
            )
        }
    }

    let ourMaxPayload = maxPayloadSize ?? (1024 * 1024)
    warnLog("[vox-establish] acceptor: starting handshake")
    let handshake = try await performAcceptorHandshake(
        link: attachment.link,
        maxPayloadSize: ourMaxPayload,
        maxConcurrentRequests: 64,
        initialChannelCredit: initialChannelCredit,
        resumable: resumable,
        metadata: metadata
    )

    let conduit = try await buildEstablishedConduit(
        role: .acceptor,
        transport: transport,
        attachment: attachment,
        peerMessageSchema: handshake.peerMessageSchema
    )
    try await conduit.setMaxFrameSize(Int(handshake.negotiated.maxPayloadSize) + 64)

    let (connection, driver, handle) = makeSessionDriverAndConnection(
        conduit: conduit,
        dispatcher: dispatcher,
        role: .acceptor,
        negotiated: handshake.negotiated,
        peerSupportsRetry: handshake.peerSupportsRetry,
        connectionAcceptor: connectionAcceptor,
        keepalive: keepalive,
        resumable: resumable,
        sessionResumeKey: handshake.sessionResumeKey,
        localRootSettings: handshake.localRootSettings,
        peerRootSettings: handshake.peerRootSettings,
        peerMessageSchema: handshake.peerMessageSchema,
        transport: transport,
        recoverAttachment: nil
    )
    return (connection, driver, handle, handshake.sessionResumeKey, handshake.peerMetadata)
}

func establishAcceptor(
    link: any Link,
    transport: ConduitKind = .bare,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    resumable: Bool = false,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, [UInt8]?, Metadata) {
    try await establishAcceptor(
        attachment: .init(link: link),
        transport: transport,
        dispatcher: dispatcher,
        connectionAcceptor: connectionAcceptor,
        maxPayloadSize: maxPayloadSize,
        initialChannelCredit: initialChannelCredit,
        keepalive: keepalive,
        resumable: resumable,
        metadata: metadata
    )
}

func establishAcceptor(
    conduit: any Link,
    transport: ConduitKind = .bare,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    resumable: Bool = false,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, [UInt8]?, Metadata) {
    try await establishAcceptor(
        link: conduit,
        transport: transport,
        dispatcher: dispatcher,
        connectionAcceptor: connectionAcceptor,
        maxPayloadSize: maxPayloadSize,
        initialChannelCredit: initialChannelCredit,
        keepalive: keepalive,
        resumable: resumable,
        metadata: metadata
    )
}
