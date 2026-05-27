import BinetteSwiftProbes
import CBinette
import XCTest

private struct VoxSwiftChannel {
}

private enum VoxSwiftOutcome: Equatable {
    case accepted(String)
    case rejected(UInt32)
}

private struct VoxSwiftCall {
    var method: UInt32
    var title: String
    var payload: [UInt8]
    var retry: UInt16?
    var outcome: VoxSwiftOutcome
    var output: VoxSwiftChannel
}

private typealias CVariantProject = @convention(c) (UnsafePointer<UInt8>?, UnsafeMutableRawPointer?) -> UnsafePointer<UInt8>?

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
        let outcomeDescriptor = arena.enumeration(
            typeID: 0xB1_0000_0000_2003,
            layout: binetteLayout(of: VoxSwiftOutcome.self),
            tag: BinetteLocalEnumTagAccessAbi(
                tag: UInt32(BINETTE_LOCAL_ACCESS_THUNK),
                direct_offset: 0,
                thunk: BinetteLocalEnumTagThunkAbi(call: outcomeTag, context: nil)
            ),
            variants: [
                BinetteLocalVariantAbi(
                    name: binetteLocalStr("accepted"),
                    index: 0,
                    project: projectAccess(outcomeProjectAcceptedBorrowed),
                    project_into: BinetteLocalVariantProjectIntoAbi(
                        call: outcomeProjectAcceptedInto,
                        context: nil
                    ),
                    drop_projected: BinetteLocalVariantDropAbi(
                        call: dropProjectedString,
                        context: nil
                    ),
                    construct: BinetteLocalVariantConstructAbi(
                        call: outcomeConstructAccepted,
                        context: nil
                    ),
                    payload: stringDescriptor
                ),
                BinetteLocalVariantAbi(
                    name: binetteLocalStr("rejected"),
                    index: 1,
                    project: projectAccess(outcomeProjectRejectedBorrowed),
                    project_into: BinetteLocalVariantProjectIntoAbi(
                        call: outcomeProjectRejectedInto,
                        context: nil
                    ),
                    drop_projected: BinetteLocalVariantDropAbi(
                        call: nil,
                        context: nil
                    ),
                    construct: BinetteLocalVariantConstructAbi(
                        call: outcomeConstructRejected,
                        context: nil
                    ),
                    payload: u32Descriptor
                ),
            ]
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
                    name: binetteLocalStr("outcome"),
                    offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.outcome)!,
                    descriptor: outcomeDescriptor
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

        let values = [
            VoxSwiftCall(
                method: 0xCAFE_BABE,
                title: "hello from vox swift",
                payload: [0, 1, 2, 3, 5, 8],
                retry: 144,
                outcome: .accepted("stream attached"),
                output: VoxSwiftChannel()
            ),
            VoxSwiftCall(
                method: 0xCAFE_BABE,
                title: "hello from vox swift",
                payload: [13, 21, 34],
                retry: nil,
                outcome: .rejected(409),
                output: VoxSwiftChannel()
            ),
        ]

        for var value in values {
            let decodedValue = try roundTrip(
                value: &value,
                imported: imported,
                schemaBundle: schemaBundle
            )

            XCTAssertEqual(decodedValue.method, value.method)
            XCTAssertEqual(decodedValue.title, value.title)
            XCTAssertEqual(decodedValue.payload, value.payload)
            XCTAssertEqual(decodedValue.retry, value.retry)
            XCTAssertEqual(decodedValue.outcome, value.outcome)
        }
    }
}

private func roundTrip(
    value: inout VoxSwiftCall,
    imported: OpaquePointer,
    schemaBundle: BinetteByteBuffer
) throws -> VoxSwiftCall {
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
        return decodedValue
}

private func projectAccess(_ thunk: @escaping CVariantProject) -> BinetteLocalVariantProjectAccessAbi {
    BinetteLocalVariantProjectAccessAbi(
        tag: UInt32(BINETTE_LOCAL_ACCESS_THUNK),
        direct_offset: 0,
        thunk: BinetteLocalVariantProjectThunkAbi(call: thunk, context: nil)
    )
}

private func outcomeTag(
    _ value: UnsafePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) -> UInt32 {
    switch UnsafeRawPointer(value!).assumingMemoryBound(to: VoxSwiftOutcome.self).pointee {
    case .accepted:
        return 0
    case .rejected:
        return 1
    }
}

private func outcomeProjectAcceptedBorrowed(
    _ value: UnsafePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) -> UnsafePointer<UInt8>? {
    nil
}

private func outcomeProjectRejectedBorrowed(
    _ value: UnsafePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) -> UnsafePointer<UInt8>? {
    nil
}

private func outcomeProjectAcceptedInto(
    _ value: UnsafePointer<UInt8>?,
    _ out: UnsafeMutablePointer<UInt8>?,
    _ outLen: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    guard outLen == MemoryLayout<String>.size else { return false }
    let outcome = UnsafeRawPointer(value!).assumingMemoryBound(to: VoxSwiftOutcome.self).pointee
    guard case let .accepted(message) = outcome else { return false }
    UnsafeMutableRawPointer(out!).assumingMemoryBound(to: String.self).initialize(to: message)
    return true
}

private func outcomeProjectRejectedInto(
    _ value: UnsafePointer<UInt8>?,
    _ out: UnsafeMutablePointer<UInt8>?,
    _ outLen: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    guard outLen == MemoryLayout<UInt32>.size else { return false }
    let outcome = UnsafeRawPointer(value!).assumingMemoryBound(to: VoxSwiftOutcome.self).pointee
    guard case let .rejected(code) = outcome else { return false }
    UnsafeMutableRawPointer(out!).assumingMemoryBound(to: UInt32.self).initialize(to: code)
    return true
}

private func dropProjectedString(
    _ value: UnsafeMutablePointer<UInt8>?,
    _ context: UnsafeMutableRawPointer?
) {
    UnsafeMutableRawPointer(value!).assumingMemoryBound(to: String.self).deinitialize(count: 1)
}

private func outcomeConstructAccepted(
    _ value: UnsafeMutablePointer<UInt8>?,
    _ payload: UnsafePointer<UInt8>?,
    _ payloadLen: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    let bytes = UnsafeBufferPointer(start: payload, count: payloadLen)
    guard let message = String(bytes: bytes, encoding: .utf8) else { return false }
    UnsafeMutableRawPointer(value!).assumingMemoryBound(to: VoxSwiftOutcome.self).initialize(to: .accepted(message))
    return true
}

private func outcomeConstructRejected(
    _ value: UnsafeMutablePointer<UInt8>?,
    _ payload: UnsafePointer<UInt8>?,
    _ payloadLen: Int,
    _ context: UnsafeMutableRawPointer?
) -> Bool {
    guard payloadLen == MemoryLayout<UInt32>.size else { return false }
    var code: UInt32 = 0
    withUnsafeMutableBytes(of: &code) { out in
        out.copyMemory(from: UnsafeRawBufferPointer(start: payload, count: payloadLen))
    }
    UnsafeMutableRawPointer(value!).assumingMemoryBound(to: VoxSwiftOutcome.self).initialize(to: .rejected(code))
    return true
}
