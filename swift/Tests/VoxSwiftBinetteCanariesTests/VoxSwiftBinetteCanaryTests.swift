import BinetteSwiftProbes
import CBinette
import CVox
import VoxSwift
import VoxSwiftGenerated
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

private struct VoxSwiftCallWithLegacyField {
    var method: UInt32
    var legacy: UInt32
    var title: String
    var payload: [UInt8]
    var retry: UInt16?
    var outcome: VoxSwiftOutcome
    var output: VoxSwiftChannel
}

private struct VoxSwiftCallReader {
    var title: String
    var outcome: VoxSwiftOutcome
    var method: UInt32
    var output: VoxSwiftChannel
    var retry: UInt16?
    var payload: [UInt8]
}

private struct VoxSwiftArgs {
    var title: String
    var retry: UInt16?
    var output: VoxSwiftChannel
}

private struct VoxSwiftReply {
    var message: String
    var retry: UInt16?
}

private typealias CVariantProject = @convention(c) (UnsafePointer<UInt8>?, UnsafeMutableRawPointer?) -> UnsafePointer<UInt8>?

final class VoxSwiftBinetteCanaryTests: XCTestCase {
    func testVoxShapedSwiftValueEncodesAndDecodesThroughBinetteLocalAccess() throws {
        let arena = BinetteCAbiDescriptorArena()
        let descriptor = voxSwiftCallDescriptor(in: arena)
        let codec = try VoxSwiftLocalCodec(descriptor: descriptor)

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
                codec: codec
            )

            XCTAssertEqual(decodedValue.method, value.method)
            XCTAssertEqual(decodedValue.title, value.title)
            XCTAssertEqual(decodedValue.payload, value.payload)
            XCTAssertEqual(decodedValue.retry, value.retry)
            XCTAssertEqual(decodedValue.outcome, value.outcome)
        }
    }

    func testSwiftValueDecodesAcrossWriterReaderSchemaBundles() throws {
        let writerArena = BinetteCAbiDescriptorArena()
        let writerDescriptor = voxSwiftCallWithLegacyDescriptor(in: writerArena)
        let writerCodec = try VoxSwiftLocalCodec(descriptor: writerDescriptor)

        let readerArena = BinetteCAbiDescriptorArena()
        let readerDescriptor = voxSwiftCallReaderDescriptor(in: readerArena)
        let readerCodec = try VoxSwiftLocalCodec(descriptor: readerDescriptor)

        var writerValue = VoxSwiftCallWithLegacyField(
            method: 7,
            legacy: 0xFFFF_FFFF,
            title: "writer schema has a legacy field",
            payload: [1, 3, 3, 7],
            retry: nil,
            outcome: .accepted("reader maps by field name"),
            output: VoxSwiftChannel()
        )

        let encoded = try writerCodec.encode(&writerValue)
        let decodedValue = try writerCodec.withSchemaBundle { writerSchema in
            try readerCodec.decode(
                encoded,
                writerSchemaBundle: writerSchema,
                as: VoxSwiftCallReader.self
            )
        }

        XCTAssertEqual(decodedValue.method, writerValue.method)
        XCTAssertEqual(decodedValue.title, writerValue.title)
        XCTAssertEqual(decodedValue.payload, writerValue.payload)
        XCTAssertEqual(decodedValue.retry, writerValue.retry)
        XCTAssertEqual(decodedValue.outcome, writerValue.outcome)
    }

    func testSwiftMethodArgsUseTupleDescriptorThroughVoxCodec() throws {
        let arena = BinetteCAbiDescriptorArena()
        let descriptor = voxSwiftArgsDescriptor(in: arena)
        let codec = try VoxSwiftLocalCodec(descriptor: descriptor)

        var args = VoxSwiftArgs(
            title: "tuple-shaped args",
            retry: 144,
            output: VoxSwiftChannel()
        )

        let encoded = try codec.encode(&args)
        let decoded = try codec.decode(encoded, as: VoxSwiftArgs.self)

        XCTAssertEqual(decoded.title, args.title)
        XCTAssertEqual(decoded.retry, args.retry)
    }

    func testSwiftDescriptorProducesVoxSchemaPayloadBytes() throws {
        let arena = BinetteCAbiDescriptorArena()
        let descriptor = voxSwiftArgsDescriptor(in: arena)
        let codec = try VoxSwiftLocalCodec(descriptor: descriptor)

        let payload = try codec.voxSchemaPayload()

        XCTAssertFalse(payload.isEmpty)
    }

    func testSwiftArgsPayloadFeedsRustVoxReceivePath() throws {
        let arena = BinetteCAbiDescriptorArena()
        let descriptor = voxSwiftArgsDescriptor(in: arena)
        let codec = try VoxSwiftLocalCodec(descriptor: descriptor)

        let schemaPayload = try codec.voxSchemaPayload()
        var args = VoxSwiftArgs(
            title: "swift rpc args",
            retry: 144,
            output: VoxSwiftChannel()
        )
        let payload = try codec.encode(&args)

        let status = schemaPayload.withUnsafeBufferPointer { schemaBuffer in
            payload.withUnsafeBufferPointer { payloadBuffer in
                vox_canary_accept_swift_args(
                    schemaBuffer.baseAddress,
                    schemaBuffer.count,
                    payloadBuffer.baseAddress,
                    payloadBuffer.count
                )
            }
        }

        XCTAssertEqual(status, VOX_STATUS_OK)
    }

    func testSwiftArgsCallReturnsRustVoxResponsePayload() throws {
        let argsArena = BinetteCAbiDescriptorArena()
        let argsDescriptor = voxSwiftArgsDescriptor(in: argsArena)
        let argsCodec = try VoxSwiftLocalCodec(descriptor: argsDescriptor)

        let schemaPayload = try argsCodec.voxSchemaPayload()
        var args = VoxSwiftArgs(
            title: "swift rpc args",
            retry: 144,
            output: VoxSwiftChannel()
        )
        let payload = try argsCodec.encode(&args)

        var responseSchemaPayload = VoxByteBuffer()
        var responsePayload = VoxByteBuffer()
        let status = schemaPayload.withUnsafeBufferPointer { schemaBuffer in
            payload.withUnsafeBufferPointer { payloadBuffer in
                vox_canary_call_swift_args(
                    schemaBuffer.baseAddress,
                    schemaBuffer.count,
                    payloadBuffer.baseAddress,
                    payloadBuffer.count,
                    &responseSchemaPayload,
                    &responsePayload
                )
            }
        }
        defer {
            vox_byte_buffer_free(responseSchemaPayload)
            vox_byte_buffer_free(responsePayload)
        }

        XCTAssertEqual(status, VOX_STATUS_OK)
        XCTAssertFalse(responseSchemaPayload.ptr == nil)
        XCTAssertFalse(responsePayload.ptr == nil)

        let responseSchemaBytes = Array(
            UnsafeBufferPointer(start: responseSchemaPayload.ptr, count: responseSchemaPayload.len)
        )
        let responseBytes = Array(
            UnsafeBufferPointer(start: responsePayload.ptr, count: responsePayload.len)
        )

        let replyArena = BinetteCAbiDescriptorArena()
        let replyDescriptor = voxSwiftReplyDescriptor(in: replyArena)
        let replyCodec = try VoxSwiftLocalCodec(descriptor: replyDescriptor)
        let reply = try replyCodec.decodeVoxPayload(
            responseBytes,
            writerSchemaPayload: responseSchemaBytes,
            as: VoxSwiftReply.self
        )

        XCTAssertEqual(reply.message, "rust vox response")
        XCTAssertEqual(reply.retry, 233)
    }

    func testSwiftArgsCallRunsThroughRustVoxDriver() throws {
        let argsArena = BinetteCAbiDescriptorArena()
        let argsDescriptor = voxSwiftArgsDescriptor(in: argsArena)
        let replyArena = BinetteCAbiDescriptorArena()
        let replyDescriptor = voxSwiftReplyDescriptor(in: replyArena)
        let methodCodec = try VoxSwiftMethodCodec(
            argsDescriptor: argsDescriptor,
            responseDescriptor: replyDescriptor
        )

        var args = VoxSwiftArgs(
            title: "swift rpc args",
            retry: 144,
            output: VoxSwiftChannel()
        )
        let request = try methodCodec.encodeArgs(&args)

        var responseSchemaPayload = VoxByteBuffer()
        var responsePayload = VoxByteBuffer()
        let status = request.schemaPayload.withUnsafeBufferPointer { schemaBuffer in
            request.payload.withUnsafeBufferPointer { payloadBuffer in
                vox_canary_driver_call_swift_args(
                    schemaBuffer.baseAddress,
                    schemaBuffer.count,
                    payloadBuffer.baseAddress,
                    payloadBuffer.count,
                    &responseSchemaPayload,
                    &responsePayload
                )
            }
        }
        defer {
            vox_byte_buffer_free(responseSchemaPayload)
            vox_byte_buffer_free(responsePayload)
        }

        XCTAssertEqual(status, VOX_STATUS_OK)
        XCTAssertFalse(responseSchemaPayload.ptr == nil)
        XCTAssertFalse(responsePayload.ptr == nil)

        let responseSchemaBytes = Array(
            UnsafeBufferPointer(start: responseSchemaPayload.ptr, count: responseSchemaPayload.len)
        )
        let responseBytes = Array(
            UnsafeBufferPointer(start: responsePayload.ptr, count: responsePayload.len)
        )

        let reply = try methodCodec.decodeResponse(
            VoxSwiftWirePayload(
                schemaPayload: responseSchemaBytes,
                payload: responseBytes
            ),
            as: VoxSwiftReply.self
        )

        XCTAssertEqual(reply.message, "rust vox response")
        XCTAssertEqual(reply.retry, 233)
    }

    func testSwiftRichCallRunsThroughRustVoxDriver() throws {
        let argsArena = BinetteCAbiDescriptorArena()
        let argsDescriptor = voxSwiftCallDescriptor(in: argsArena)
        let replyArena = BinetteCAbiDescriptorArena()
        let replyDescriptor = voxSwiftReplyDescriptor(in: replyArena)
        let methodCodec = try VoxSwiftMethodCodec(
            argsDescriptor: argsDescriptor,
            responseDescriptor: replyDescriptor
        )

        var call = VoxSwiftCall(
            method: 0xCAFE_BABE,
            title: "hello from vox swift",
            payload: [0, 1, 2, 3, 5, 8],
            retry: 144,
            outcome: .accepted("stream attached"),
            output: VoxSwiftChannel()
        )
        let request = try methodCodec.encodeArgs(&call)

        var responseSchemaPayload = VoxByteBuffer()
        var responsePayload = VoxByteBuffer()
        let status = request.schemaPayload.withUnsafeBufferPointer { schemaBuffer in
            request.payload.withUnsafeBufferPointer { payloadBuffer in
                vox_canary_driver_call_swift_rich(
                    schemaBuffer.baseAddress,
                    schemaBuffer.count,
                    payloadBuffer.baseAddress,
                    payloadBuffer.count,
                    &responseSchemaPayload,
                    &responsePayload
                )
            }
        }
        defer {
            vox_byte_buffer_free(responseSchemaPayload)
            vox_byte_buffer_free(responsePayload)
        }

        XCTAssertEqual(status, VOX_STATUS_OK)
        XCTAssertFalse(responseSchemaPayload.ptr == nil)
        XCTAssertFalse(responsePayload.ptr == nil)

        let responseSchemaBytes = Array(
            UnsafeBufferPointer(start: responseSchemaPayload.ptr, count: responseSchemaPayload.len)
        )
        let responseBytes = Array(
            UnsafeBufferPointer(start: responsePayload.ptr, count: responsePayload.len)
        )

        let reply = try methodCodec.decodeResponse(
            VoxSwiftWirePayload(
                schemaPayload: responseSchemaBytes,
                payload: responseBytes
            ),
            as: VoxSwiftReply.self
        )

        XCTAssertEqual(reply.message, "3405691582:hello from vox swift:6:accepted:stream attached")
        XCTAssertEqual(reply.retry, 144)
    }

    func testGeneratedSwiftCanaryRunsThroughRustVoxDriver() throws {
        let methodCodec = try SwiftCanaryMethodCodecs().submit()

        var args = SwiftCanarySubmitArgs(
            call: SwiftCall(
                method: 0xCAFE_BABE,
                nonce: 0x0123_4567_89AB_CDEF,
                ratio: 1.5,
                title: "hello from generated swift",
                note: "optional generated note",
                payload: [2, 3, 5, 7],
                retry: 377,
                outcome: .accepted("generated descriptor"),
                output: VoxSwift.VoxSwiftChannel()
            )
        )
        let request = try methodCodec.encodeArgs(&args)

        var responseSchemaPayload = VoxByteBuffer()
        var responsePayload = VoxByteBuffer()
        let status = request.schemaPayload.withUnsafeBufferPointer { schemaBuffer in
            request.payload.withUnsafeBufferPointer { payloadBuffer in
                vox_canary_driver_call_generated_swift_submit(
                    schemaBuffer.baseAddress,
                    schemaBuffer.count,
                    payloadBuffer.baseAddress,
                    payloadBuffer.count,
                    &responseSchemaPayload,
                    &responsePayload
                )
            }
        }
        defer {
            vox_byte_buffer_free(responseSchemaPayload)
            vox_byte_buffer_free(responsePayload)
        }

        XCTAssertEqual(status, VOX_STATUS_OK)

        let responseSchemaBytes = Array(
            UnsafeBufferPointer(start: responseSchemaPayload.ptr, count: responseSchemaPayload.len)
        )
        let responseBytes = Array(
            UnsafeBufferPointer(start: responsePayload.ptr, count: responsePayload.len)
        )

        let reply = try methodCodec.decodeResponse(
            VoxSwiftWirePayload(
                schemaPayload: responseSchemaBytes,
                payload: responseBytes
            ),
            as: SwiftReply.self
        )

        XCTAssertEqual(
            reply.message,
            "3405691582:81985529216486895:1.5:hello from generated swift:optional generated note:4:accepted:generated descriptor"
        )
        XCTAssertEqual(reply.retry, 377)
    }
}

private func roundTrip(
    value: inout VoxSwiftCall,
    codec: VoxSwiftLocalCodec
) throws -> VoxSwiftCall {
    let encoded = try codec.encode(&value)
    return try codec.decode(encoded, as: VoxSwiftCall.self)
}

private func voxSwiftCallDescriptor(
    in arena: BinetteCAbiDescriptorArena
) -> UnsafePointer<BinetteLocalDescriptorAbi> {
    let parts = commonDescriptors(in: arena)
    return arena.structure(
        typeID: 0xB1_0000_0000_2000,
        layout: binetteLayout(of: VoxSwiftCall.self),
        fields: [
            BinetteLocalFieldAbi(
                name: binetteLocalStr("method"),
                offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.method)!,
                descriptor: parts.u32
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("title"),
                offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.title)!,
                descriptor: parts.string
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("payload"),
                offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.payload)!,
                descriptor: parts.bytes
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("retry"),
                offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.retry)!,
                descriptor: parts.optionalU16
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("outcome"),
                offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.outcome)!,
                descriptor: parts.outcome
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("output"),
                offset: MemoryLayout<VoxSwiftCall>.offset(of: \VoxSwiftCall.output)!,
                descriptor: parts.channel
            ),
        ]
    )
}

private func voxSwiftCallWithLegacyDescriptor(
    in arena: BinetteCAbiDescriptorArena
) -> UnsafePointer<BinetteLocalDescriptorAbi> {
    let parts = commonDescriptors(in: arena)
    return arena.structure(
        typeID: 0xB1_0000_0000_2100,
        layout: binetteLayout(of: VoxSwiftCallWithLegacyField.self),
        fields: [
            BinetteLocalFieldAbi(
                name: binetteLocalStr("method"),
                offset: MemoryLayout<VoxSwiftCallWithLegacyField>.offset(of: \VoxSwiftCallWithLegacyField.method)!,
                descriptor: parts.u32
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("legacy"),
                offset: MemoryLayout<VoxSwiftCallWithLegacyField>.offset(of: \VoxSwiftCallWithLegacyField.legacy)!,
                descriptor: parts.u32
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("title"),
                offset: MemoryLayout<VoxSwiftCallWithLegacyField>.offset(of: \VoxSwiftCallWithLegacyField.title)!,
                descriptor: parts.string
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("payload"),
                offset: MemoryLayout<VoxSwiftCallWithLegacyField>.offset(of: \VoxSwiftCallWithLegacyField.payload)!,
                descriptor: parts.bytes
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("retry"),
                offset: MemoryLayout<VoxSwiftCallWithLegacyField>.offset(of: \VoxSwiftCallWithLegacyField.retry)!,
                descriptor: parts.optionalU16
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("outcome"),
                offset: MemoryLayout<VoxSwiftCallWithLegacyField>.offset(of: \VoxSwiftCallWithLegacyField.outcome)!,
                descriptor: parts.outcome
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("output"),
                offset: MemoryLayout<VoxSwiftCallWithLegacyField>.offset(of: \VoxSwiftCallWithLegacyField.output)!,
                descriptor: parts.channel
            ),
        ]
    )
}

private func voxSwiftCallReaderDescriptor(
    in arena: BinetteCAbiDescriptorArena
) -> UnsafePointer<BinetteLocalDescriptorAbi> {
    let parts = commonDescriptors(in: arena)
    return arena.structure(
        typeID: 0xB1_0000_0000_2101,
        layout: binetteLayout(of: VoxSwiftCallReader.self),
        fields: [
            BinetteLocalFieldAbi(
                name: binetteLocalStr("title"),
                offset: MemoryLayout<VoxSwiftCallReader>.offset(of: \VoxSwiftCallReader.title)!,
                descriptor: parts.string
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("outcome"),
                offset: MemoryLayout<VoxSwiftCallReader>.offset(of: \VoxSwiftCallReader.outcome)!,
                descriptor: parts.outcome
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("method"),
                offset: MemoryLayout<VoxSwiftCallReader>.offset(of: \VoxSwiftCallReader.method)!,
                descriptor: parts.u32
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("output"),
                offset: MemoryLayout<VoxSwiftCallReader>.offset(of: \VoxSwiftCallReader.output)!,
                descriptor: parts.channel
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("retry"),
                offset: MemoryLayout<VoxSwiftCallReader>.offset(of: \VoxSwiftCallReader.retry)!,
                descriptor: parts.optionalU16
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("payload"),
                offset: MemoryLayout<VoxSwiftCallReader>.offset(of: \VoxSwiftCallReader.payload)!,
                descriptor: parts.bytes
            ),
        ]
    )
}

private func voxSwiftArgsDescriptor(
    in arena: BinetteCAbiDescriptorArena
) -> UnsafePointer<BinetteLocalDescriptorAbi> {
    let parts = commonDescriptors(in: arena)
    return arena.tuple(
        typeID: 0xB1_0000_0000_2200,
        layout: binetteLayout(of: VoxSwiftArgs.self),
        fields: [
            BinetteLocalFieldAbi(
                name: binetteLocalStr("0"),
                offset: MemoryLayout<VoxSwiftArgs>.offset(of: \VoxSwiftArgs.title)!,
                descriptor: parts.string
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("1"),
                offset: MemoryLayout<VoxSwiftArgs>.offset(of: \VoxSwiftArgs.retry)!,
                descriptor: parts.optionalU16
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("2"),
                offset: MemoryLayout<VoxSwiftArgs>.offset(of: \VoxSwiftArgs.output)!,
                descriptor: parts.channel
            ),
        ]
    )
}

private func voxSwiftReplyDescriptor(
    in arena: BinetteCAbiDescriptorArena
) -> UnsafePointer<BinetteLocalDescriptorAbi> {
    let parts = commonDescriptors(in: arena)
    return arena.structure(
        typeID: 0xB1_0000_0000_2300,
        layout: binetteLayout(of: VoxSwiftReply.self),
        fields: [
            BinetteLocalFieldAbi(
                name: binetteLocalStr("message"),
                offset: MemoryLayout<VoxSwiftReply>.offset(of: \VoxSwiftReply.message)!,
                descriptor: parts.string
            ),
            BinetteLocalFieldAbi(
                name: binetteLocalStr("retry"),
                offset: MemoryLayout<VoxSwiftReply>.offset(of: \VoxSwiftReply.retry)!,
                descriptor: parts.optionalU16
            ),
        ]
    )
}

private struct CommonDescriptors {
    let u32: UnsafePointer<BinetteLocalDescriptorAbi>
    let string: UnsafePointer<BinetteLocalDescriptorAbi>
    let bytes: UnsafePointer<BinetteLocalDescriptorAbi>
    let optionalU16: UnsafePointer<BinetteLocalDescriptorAbi>
    let outcome: UnsafePointer<BinetteLocalDescriptorAbi>
    let channel: UnsafePointer<BinetteLocalDescriptorAbi>
}

private func commonDescriptors(in arena: BinetteCAbiDescriptorArena) -> CommonDescriptors {
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
        layout: binetteLayout(of: VoxSwiftChannel.self),
        metadataFields: [
            arena.externalMetadataField(
                "direction",
                arena.externalMetadataString("tx")
            ),
            arena.externalMetadataField(
                "element",
                arena.externalMetadataTypeRef(u32Descriptor)
            ),
        ]
    )
    return CommonDescriptors(
        u32: u32Descriptor,
        string: stringDescriptor,
        bytes: bytesDescriptor,
        optionalU16: optionalU16Descriptor,
        outcome: outcomeDescriptor,
        channel: channelDescriptor
    )
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
