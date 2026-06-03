import Foundation

/// Commands from ConnectionHandle to Driver.
enum HandleCommand: Sendable {
    case call(
        requestId: UInt64,
        methodId: UInt64,
        metadata: Metadata,
        payload: [UInt8],
        channels: [UInt64],
        timeout: TimeInterval?,
        responseTx: @Sendable (Result<[UInt8], ConnectionError>) -> Void,
        schemaInfo: ClientSchemaInfo?
    )
}
