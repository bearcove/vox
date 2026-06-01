import PhonSchema

// Retry support is advertised and tracked through metadata (`Value`) entries:
// a `vox-retry-support` u64 version flag and a `vox-operation-id` u64 the peer
// echoes for idempotent retries.

public let retrySupportMetadataKey = "vox-retry-support"
public let operationIdMetadataKey = "vox-operation-id"
public let retrySupportVersion: UInt64 = 1

public func appendRetrySupportMetadata(_ metadata: Metadata) -> Metadata {
    guard !metadataSupportsRetry(metadata) else { return metadata }
    return metadata.metaSetting(retrySupportMetadataKey, metaU64Value(retrySupportVersion))
}

public func metadataSupportsRetry(_ metadata: Metadata) -> Bool {
    metadata.metaU64(retrySupportMetadataKey) == retrySupportVersion
}

public func metadataOperationId(_ metadata: Metadata) -> UInt64? {
    metadata.metaU64(operationIdMetadataKey)
}

public func ensureOperationId(_ metadata: Metadata, operationId: UInt64) -> Metadata {
    guard metadataOperationId(metadata) == nil else { return metadata }
    return metadata.metaSetting(operationIdMetadataKey, metaU64Value(operationId))
}

public func replacingOperationId(_ metadata: Metadata, operationId: UInt64) -> Metadata {
    metadata.metaSetting(operationIdMetadataKey, metaU64Value(operationId))
}
