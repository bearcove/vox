import BinetteSwiftProbes
import CBinette
import CVox

public enum VoxSwiftBinetteError: Error, Equatable {
    case importDescriptor(Int32)
    case schemaBundle(Int32)
    case schemaPayload(Int32)
    case encode(Int32)
    case decode(Int32)
    case methodMismatch(expected: UInt64, actual: UInt64)
}

public struct VoxSwiftWirePayload {
    public var methodId: UInt64
    public var schemaPayload: [UInt8]
    public var payload: [UInt8]

    public init(methodId: UInt64, schemaPayload: [UInt8], payload: [UInt8]) {
        self.methodId = methodId
        self.schemaPayload = schemaPayload
        self.payload = payload
    }
}

public struct VoxSwiftChannel {
    public init() {}
}

public final class VoxSwiftMethodCodec {
    public let methodId: UInt64
    private let argsCodec: VoxSwiftLocalCodec
    private let responseCodec: VoxSwiftLocalCodec

    public init(
        methodId: UInt64,
        argsDescriptor: UnsafePointer<BinetteLocalDescriptorAbi>,
        responseDescriptor: UnsafePointer<BinetteLocalDescriptorAbi>
    ) throws {
        self.methodId = methodId
        argsCodec = try VoxSwiftLocalCodec(descriptor: argsDescriptor)
        responseCodec = try VoxSwiftLocalCodec(descriptor: responseDescriptor)
    }

    public func encodeArgs<T>(_ value: inout T) throws -> VoxSwiftWirePayload {
        VoxSwiftWirePayload(
            methodId: methodId,
            schemaPayload: try argsCodec.voxSchemaPayload(),
            payload: try argsCodec.encode(&value)
        )
    }

    public func encodeResponse<T>(_ value: inout T) throws -> VoxSwiftWirePayload {
        VoxSwiftWirePayload(
            methodId: methodId,
            schemaPayload: try responseCodec.voxSchemaPayload(),
            payload: try responseCodec.encode(&value)
        )
    }

    public func wrapResponse(schemaPayload: [UInt8], payload: [UInt8]) -> VoxSwiftWirePayload {
        VoxSwiftWirePayload(methodId: methodId, schemaPayload: schemaPayload, payload: payload)
    }

    public func decodeResponse<T>(
        _ response: VoxSwiftWirePayload,
        as _: T.Type
    ) throws -> T {
        guard response.methodId == methodId else {
            throw VoxSwiftBinetteError.methodMismatch(expected: methodId, actual: response.methodId)
        }
        return try responseCodec.decodeVoxPayload(
            response.payload,
            writerSchemaPayload: response.schemaPayload,
            as: T.self
        )
    }

}

public final class VoxSwiftLocalCodec {
    private let handle: OpaquePointer
    private let schemaBundle: BinetteByteBuffer

    public init(descriptor: UnsafePointer<BinetteLocalDescriptorAbi>) throws {
        var imported: OpaquePointer?
        let importStatus = binette_local_descriptor_import(descriptor, &imported)
        guard importStatus == BINETTE_STATUS_OK, let imported else {
            throw VoxSwiftBinetteError.importDescriptor(importStatus)
        }

        var bundle = BinetteByteBuffer()
        let schemaStatus = binette_local_descriptor_synthetic_schema_bundle(imported, &bundle)
        guard schemaStatus == BINETTE_STATUS_OK else {
            binette_local_descriptor_free(imported)
            throw VoxSwiftBinetteError.schemaBundle(schemaStatus)
        }

        handle = imported
        schemaBundle = bundle
    }

    deinit {
        binette_byte_buffer_free(schemaBundle)
        binette_local_descriptor_free(handle)
    }

    public func encode<T>(_ value: inout T) throws -> [UInt8] {
        var encoded = BinetteByteBuffer()
        let status = withUnsafePointer(to: &value) { pointer in
            binette_local_encode_with_schema_bundle(
                handle,
                UnsafePointer(schemaBundle.ptr),
                schemaBundle.len,
                UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self),
                &encoded
            )
        }
        guard status == BINETTE_STATUS_OK else {
            throw VoxSwiftBinetteError.encode(status)
        }
        defer { binette_byte_buffer_free(encoded) }
        return Array(UnsafeBufferPointer(start: encoded.ptr, count: encoded.len))
    }

    public func decode<T>(_ bytes: [UInt8], as _: T.Type) throws -> T {
        let bundle = schemaBundle
        let decoded = UnsafeMutablePointer<T>.allocate(capacity: 1)
        let status = bytes.withUnsafeBufferPointer { buffer in
            binette_local_decode_with_schema_bundles(
                handle,
                UnsafePointer(bundle.ptr),
                bundle.len,
                UnsafePointer(bundle.ptr),
                bundle.len,
                buffer.baseAddress,
                buffer.count,
                UnsafeMutableRawPointer(decoded).assumingMemoryBound(to: UInt8.self)
            )
        }
        guard status == BINETTE_STATUS_OK else {
            decoded.deallocate()
            throw VoxSwiftBinetteError.decode(status)
        }
        let value = decoded.move()
        decoded.deallocate()
        return value
    }

    public func decode<T>(_ bytes: [UInt8], writer: VoxSwiftLocalCodec, as _: T.Type) throws -> T {
        try writer.withSchemaBundle { writerSchema in
            try decode(bytes, writerSchemaBundle: writerSchema, as: T.self)
        }
    }

    public func decode<T>(
        _ bytes: [UInt8],
        writerSchemaBundle: UnsafeBufferPointer<UInt8>,
        as _: T.Type
    ) throws -> T {
        try decode(
            bytes,
            writerSchemaBundlePtr: writerSchemaBundle.baseAddress,
            writerSchemaBundleLen: writerSchemaBundle.count,
            as: T.self
        )
    }

    private func decode<T>(
        _ bytes: [UInt8],
        writerSchemaBundlePtr: UnsafePointer<UInt8>?,
        writerSchemaBundleLen: Int,
        as _: T.Type
    ) throws -> T {
        let readerBundle = schemaBundle
        let decoded = UnsafeMutablePointer<T>.allocate(capacity: 1)
        let status = bytes.withUnsafeBufferPointer { buffer in
            binette_local_decode_with_schema_bundles(
                handle,
                writerSchemaBundlePtr,
                writerSchemaBundleLen,
                UnsafePointer(readerBundle.ptr),
                readerBundle.len,
                buffer.baseAddress,
                buffer.count,
                UnsafeMutableRawPointer(decoded).assumingMemoryBound(to: UInt8.self)
            )
        }
        guard status == BINETTE_STATUS_OK else {
            decoded.deallocate()
            throw VoxSwiftBinetteError.decode(status)
        }
        let value = decoded.move()
        decoded.deallocate()
        return value
    }

    public func decodeVoxPayload<T>(
        _ bytes: [UInt8],
        writerSchemaPayload: [UInt8],
        as _: T.Type
    ) throws -> T {
        let writerSchemaBundle = try Self.binetteSchemaBundle(fromVoxSchemaPayload: writerSchemaPayload)
        return try writerSchemaBundle.withUnsafeBufferPointer { writerSchema in
            try decode(bytes, writerSchemaBundle: writerSchema, as: T.self)
        }
    }

    public func withSchemaBundle<R>(_ body: (UnsafeBufferPointer<UInt8>) throws -> R) rethrows -> R {
        try body(UnsafeBufferPointer(start: schemaBundle.ptr, count: schemaBundle.len))
    }

    public func voxSchemaPayload() throws -> [UInt8] {
        var payload = VoxByteBuffer()
        let status = vox_schema_payload_from_binette_schema_bundle(
            UnsafePointer(schemaBundle.ptr),
            schemaBundle.len,
            &payload
        )
        guard status == VOX_STATUS_OK else {
            throw VoxSwiftBinetteError.schemaPayload(status)
        }
        defer { vox_byte_buffer_free(payload) }
        return Array(UnsafeBufferPointer(start: payload.ptr, count: payload.len))
    }

    public static func binetteSchemaBundle(fromVoxSchemaPayload schemaPayload: [UInt8]) throws -> [UInt8] {
        var bundle = VoxByteBuffer()
        let status = schemaPayload.withUnsafeBufferPointer { buffer in
            vox_binette_schema_bundle_from_schema_payload(
                buffer.baseAddress,
                buffer.count,
                &bundle
            )
        }
        guard status == VOX_STATUS_OK else {
            throw VoxSwiftBinetteError.schemaPayload(status)
        }
        defer { vox_byte_buffer_free(bundle) }
        return Array(UnsafeBufferPointer(start: bundle.ptr, count: bundle.len))
    }
}
