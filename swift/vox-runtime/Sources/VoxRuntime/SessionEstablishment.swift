import Foundation

struct SessionHandshakeResult {
    let negotiated: Negotiated
    let localRootSettings: ConnectionSettings
    let peerRootSettings: ConnectionSettings
    let peerMetadata: Metadata
    /// The peer's advertised Message schema closure, used to build the conduit's
    /// compatibility decoder.
    let peerMessageSchema: [UInt8]
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
        let reason = "unsupported message compatibility plan"
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
    metadata: Metadata = .null
) async throws -> SessionHandshakeResult {
    traceLog(.handshake, "initiator sending Hello")
    let ourSettings = ConnectionSettings(
        parity: .odd, maxConcurrentRequests: maxConcurrentRequests,
        initialChannelCredit: initialChannelCredit)
    let hello = Hello(
        parity: ourSettings.parity,
        connectionSettings: ourSettings,
        messagePayloadSchema: Data(localMessagePayloadSchema),
        metadata: metadata
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
    metadata: Metadata = .null
) async throws -> SessionHandshakeResult {
    let peerHello: Hello
    switch try await recvHandshake(link) {
    case .hello(let hello):
        traceLog(.handshake, "acceptor received Hello")
        peerHello = hello
    default:
        throw ConnectionError.handshakeFailed("expected Hello")
    }

    let peerSchema = [UInt8](peerHello.messagePayloadSchema)
    try await requireIdentityMessageSchema(peerSchema, on: link)

    let ourSettings = ConnectionSettings(
        parity: oppositeParity(peerHello.parity),
        maxConcurrentRequests: maxConcurrentRequests,
        initialChannelCredit: initialChannelCredit
    )
    let helloYourself = HelloYourself(
        connectionSettings: ourSettings,
        messagePayloadSchema: Data(localMessagePayloadSchema),
        metadata: metadata
    )
    try await sendHandshake(link, .helloYourself(helloYourself))
    traceLog(.handshake, "acceptor sent HelloYourself")

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
        localRootSettings: ourSettings,
        peerRootSettings: peerHello.connectionSettings,
        peerMetadata: peerHello.metadata,
        peerMessageSchema: peerSchema
    )
}

// r[impl transport.prologue.post-accept]
func buildEstablishedConduit(
    role: Role,
    attachment: LinkAttachment,
    peerMessageSchema: [UInt8]
) async throws -> any Conduit {
    let _ = role
    return BareConduit(link: attachment.link, peerMessageSchema: peerMessageSchema)
}


func establishInitiator(
    attachment: LinkAttachment,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, Metadata) {
    warnLog("[vox-establish] initiator: starting handshake")
    let ourMaxPayload = maxPayloadSize ?? (1024 * 1024)
    let handshake = try await performInitiatorHandshake(
        link: attachment.link,
        maxPayloadSize: ourMaxPayload,
        maxConcurrentRequests: 64,
        initialChannelCredit: initialChannelCredit,
        metadata: metadata
    )
    warnLog("[vox-establish] initiator: handshake done")

    let conduit = try await buildEstablishedConduit(
        role: .initiator,
        attachment: attachment,
        peerMessageSchema: handshake.peerMessageSchema
    )
    try await conduit.setMaxFrameSize(Int(handshake.negotiated.maxPayloadSize) + 64)

    let (connection, driver, handle) = makeSessionDriverAndConnection(
        conduit: conduit,
        dispatcher: dispatcher,
        role: .initiator,
        negotiated: handshake.negotiated,
        connectionAcceptor: connectionAcceptor,
        keepalive: keepalive,
        localRootSettings: handshake.localRootSettings,
        peerRootSettings: handshake.peerRootSettings,
        peerMessageSchema: handshake.peerMessageSchema
    )
    return (connection, driver, handle, handshake.peerMetadata)
}

func establishInitiator(
    link: any Link,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, Metadata) {
    try await establishInitiator(
        attachment: .initiator(link),
        dispatcher: dispatcher,
        connectionAcceptor: connectionAcceptor,
        maxPayloadSize: maxPayloadSize,
        initialChannelCredit: initialChannelCredit,
        keepalive: keepalive,
        metadata: metadata
    )
}

func establishInitiator(
    conduit: any Link,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, Metadata) {
    try await establishInitiator(
        link: conduit,
        dispatcher: dispatcher,
        connectionAcceptor: connectionAcceptor,
        maxPayloadSize: maxPayloadSize,
        initialChannelCredit: initialChannelCredit,
        keepalive: keepalive,
        metadata: metadata
    )
}

// r[impl transport.prologue.first-payload]
// r[impl transport.prologue.post-accept]
func establishAcceptor(
    attachment: LinkAttachment,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, Metadata) {
    warnLog("[vox-establish] acceptor: prologueComplete=\(attachment.hasCompletedPrologue)")
    if !attachment.hasCompletedPrologue {
        warnLog("[vox-establish] acceptor: running link prologue")
        try await performAcceptorLinkPrologue(link: attachment.link)
        warnLog("[vox-establish] acceptor: prologue done")
    }

    let ourMaxPayload = maxPayloadSize ?? (1024 * 1024)
    warnLog("[vox-establish] acceptor: starting handshake")
    let handshake = try await performAcceptorHandshake(
        link: attachment.link,
        maxPayloadSize: ourMaxPayload,
        maxConcurrentRequests: 64,
        initialChannelCredit: initialChannelCredit,
        metadata: metadata
    )

    let conduit = try await buildEstablishedConduit(
        role: .acceptor,
        attachment: attachment,
        peerMessageSchema: handshake.peerMessageSchema
    )
    try await conduit.setMaxFrameSize(Int(handshake.negotiated.maxPayloadSize) + 64)

    let (connection, driver, handle) = makeSessionDriverAndConnection(
        conduit: conduit,
        dispatcher: dispatcher,
        role: .acceptor,
        negotiated: handshake.negotiated,
        connectionAcceptor: connectionAcceptor,
        keepalive: keepalive,
        localRootSettings: handshake.localRootSettings,
        peerRootSettings: handshake.peerRootSettings,
        peerMessageSchema: handshake.peerMessageSchema
    )
    return (connection, driver, handle, handshake.peerMetadata)
}

func establishAcceptor(
    link: any Link,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, Metadata) {
    try await establishAcceptor(
        attachment: .init(link: link),
        dispatcher: dispatcher,
        connectionAcceptor: connectionAcceptor,
        maxPayloadSize: maxPayloadSize,
        initialChannelCredit: initialChannelCredit,
        keepalive: keepalive,
        metadata: metadata
    )
}

func establishAcceptor(
    conduit: any Link,
    dispatcher: any ServiceDispatcher,
    connectionAcceptor: (any ConnectionAcceptor)? = nil,
    maxPayloadSize: UInt32? = nil,
    initialChannelCredit: UInt32 = 16,
    keepalive: SessionKeepaliveConfig? = nil,
    metadata: Metadata = .null
) async throws -> (Connection, Driver, SessionHandle, Metadata) {
    try await establishAcceptor(
        link: conduit,
        dispatcher: dispatcher,
        connectionAcceptor: connectionAcceptor,
        maxPayloadSize: maxPayloadSize,
        initialChannelCredit: initialChannelCredit,
        keepalive: keepalive,
        metadata: metadata
    )
}
