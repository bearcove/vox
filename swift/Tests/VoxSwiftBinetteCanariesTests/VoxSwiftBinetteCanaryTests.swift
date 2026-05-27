import BinetteSwiftProbes
import CBinette
import XCTest

private struct VoxSwiftChannel {
    var raw: UInt64
}

private struct VoxSwiftCall {
    var method: UInt32
    var title: String
    var payload: [UInt8]
    var retry: UInt16?
    var output: VoxSwiftChannel
}

final class VoxSwiftBinetteCanaryTests: XCTestCase {
    func testVoxShapedSwiftValueImportsThroughBinetteLocalAccess() throws {
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
        binette_local_descriptor_free(imported)
    }
}
