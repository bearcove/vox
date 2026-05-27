import BinetteSwiftProbes
import CBinette
import XCTest

private struct VoxSwiftChannel {
}

private struct VoxSwiftCall {
    var method: UInt32
    var title: String
    var payload: [UInt8]
    var retry: UInt16?
    var output: VoxSwiftChannel
}

final class VoxSwiftBinetteCanaryTests: XCTestCase {
    func testVoxShapedSwiftValueEncodesAndDecodesThroughBinetteLocalAccess() throws {
        let arena = BinetteCAbiDescriptorArena()

        let u32Descriptor = arena.plain(typeID: binette_primitive_u32_type_id(), UInt32.self)
        let stringDescriptor = arena.string()
        let bytesDescriptor = arena.bytes()
        let u16Descriptor = arena.plain(typeID: binette_primitive_u16_type_id(), UInt16.self)
        let optionalU16Descriptor = arena.option(
            typeID: 0xB1_0000_0000_2001,
            layout: binetteLayout(of: UInt16?.self),
            some: u16Descriptor,
            representation: binetteDirectOptionalU16Representation()
        )
        let channelDescriptor = arena.externalAttachment(
            typeID: 0xB1_0000_0000_2002,
            kind: "vox.channel",
            layout: binetteLayout(of: VoxSwiftChannel.self)
        )

        let descriptor = arena.structure(
            typeID: 0xB1_0000_0000_2000,
            layout: binetteLayout(of: VoxSwiftCall.self),
            fields: [
                BinetteLocalFieldAbi(
                    name: binetteLocalStr("method"),
                    offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.method)!,
                    descriptor: u32Descriptor
                ),
                BinetteLocalFieldAbi(
                    name: binetteLocalStr("title"),
                    offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.title)!,
                    descriptor: stringDescriptor
                ),
                BinetteLocalFieldAbi(
                    name: binetteLocalStr("payload"),
                    offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.payload)!,
                    descriptor: bytesDescriptor
                ),
                BinetteLocalFieldAbi(
                    name: binetteLocalStr("retry"),
                    offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.retry)!,
                    descriptor: optionalU16Descriptor
                ),
                BinetteLocalFieldAbi(
                    name: binetteLocalStr("output"),
                    offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.output)!,
                    descriptor: channelDescriptor
                ),
            ]
        )

        var handle: OpaquePointer?
        let status = binette_local_descriptor_import(descriptor, &handle)
        XCTAssertEqual(status, BINETTE_STATUS_OK)
        let imported = try XCTUnwrap(handle)
        defer { binette_local_descriptor_free(imported) }

        var schemaBundle = BinetteByteBuffer()
        let schemaStatus = binette_local_descriptor_synthetic_schema_bundle(imported, &schemaBundle)
        XCTAssertEqual(schemaStatus, BINETTE_STATUS_OK)
        defer { binette_byte_buffer_free(schemaBundle) }

        var value = VoxSwiftCall(
            method: 0xCAFE_BABE,
            title: "hello from vox swift",
            payload: [0, 1, 2, 3, 5, 8],
            retry: 144,
            output: VoxSwiftChannel()
        )
        var encoded = BinetteByteBuffer()
        let encodeStatus = withUnsafePointer(to: &value) { pointer in
            binette_local_encode_with_schema_bundle(
                imported,
                UnsafePointer(schemaBundle.ptr),
                schemaBundle.len,
                UnsafeRawPointer(pointer).assumingMemoryBound(to: UInt8.self),
                &encoded
            )
        }
        XCTAssertEqual(encodeStatus, BINETTE_STATUS_OK)
        defer { binette_byte_buffer_free(encoded) }

        let decoded = UnsafeMutablePointer<VoxSwiftCall>.allocate(capacity: 1)
        let decodeStatus = binette_local_decode_with_schema_bundles(
            imported,
            UnsafePointer(schemaBundle.ptr),
            schemaBundle.len,
            UnsafePointer(schemaBundle.ptr),
            schemaBundle.len,
            UnsafePointer(encoded.ptr),
            encoded.len,
            UnsafeMutableRawPointer(decoded).assumingMemoryBound(to: UInt8.self)
        )
        XCTAssertEqual(decodeStatus, BINETTE_STATUS_OK)
        let decodedValue = decoded.move()
        decoded.deallocate()

        XCTAssertEqual(decodedValue.method, value.method)
        XCTAssertEqual(decodedValue.title, value.title)
        XCTAssertEqual(decodedValue.payload, value.payload)
        XCTAssertEqual(decodedValue.retry, value.retry)
    }
}
