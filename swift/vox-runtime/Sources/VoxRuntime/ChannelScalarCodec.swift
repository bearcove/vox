@preconcurrency import NIOCore

// Temporary channel-element scalar codecs over `ByteBuffer`, in the phon scalar wire
// form (fixed-width little-endian; `String` = u32 LE length + UTF-8). The phon channel
// payload path (Tx/Rx → element descriptor program) is not migrated yet, so these keep
// the channel test scenarios compiling. Echo + non-channel scenarios don't use them.
// TODO(phon): replace with the generated channel element codec from PhonChannelMeta.

public func encodeI32(_ v: Int32, into buf: inout ByteBuffer) {
    buf.writeInteger(v, endianness: .little)
}
public func decodeI32(from buf: inout ByteBuffer) throws -> Int32 {
    guard let v = buf.readInteger(endianness: .little, as: Int32.self) else {
        throw VoxRuntimeError.decodeError("i32")
    }
    return v
}

public func encodeI64(_ v: Int64, into buf: inout ByteBuffer) {
    buf.writeInteger(v, endianness: .little)
}
public func decodeI64(from buf: inout ByteBuffer) throws -> Int64 {
    guard let v = buf.readInteger(endianness: .little, as: Int64.self) else {
        throw VoxRuntimeError.decodeError("i64")
    }
    return v
}

public func encodeString(_ v: String, into buf: inout ByteBuffer) {
    let bytes = Array(v.utf8)
    buf.writeInteger(UInt32(bytes.count), endianness: .little)
    buf.writeBytes(bytes)
}
public func decodeString(from buf: inout ByteBuffer) throws -> String {
    guard let len = buf.readInteger(endianness: .little, as: UInt32.self),
        let bytes = buf.readBytes(length: Int(len)),
        let s = String(bytes: bytes, encoding: .utf8)
    else {
        throw VoxRuntimeError.decodeError("string")
    }
    return s
}
