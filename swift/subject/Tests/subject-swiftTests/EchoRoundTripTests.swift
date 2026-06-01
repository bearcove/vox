import PhonEngine
import Testing

@testable import subject_swift

// Validates the generated service typed path at runtime: the per-method args/response
// descriptors lower (no trap at global init) and encode→decode round-trips through the
// cached MemPrograms. This is the in-process equivalent of an echo RPC's payload codec.

@Test func echoArgsRoundTrip() throws {
    var args = "hello world"
    let payload = withUnsafeBytes(of: &args) {
        encodeWith(testbed_echo_ArgsEncodeProgram, $0.baseAddress!)
    }
    let raw = UnsafeMutableRawPointer.allocate(
        byteCount: MemoryLayout<String>.size, alignment: MemoryLayout<String>.alignment)
    defer { raw.deallocate() }
    try decodeInto(testbed_echo_ArgsDecodeProgram, payload, raw)
    let decoded = raw.assumingMemoryBound(to: String.self).move()
    #expect(decoded == "hello world")
}

@Test func echoResponseRoundTrip() throws {
    var resp: Result<String, VoxError<Infallible>> = .success("HELLO WORLD")
    let payload = withUnsafeBytes(of: &resp) {
        encodeWith(testbed_echo_ResponseEncodeProgram, $0.baseAddress!)
    }
    let raw = UnsafeMutableRawPointer.allocate(
        byteCount: MemoryLayout<Result<String, VoxError<Infallible>>>.size,
        alignment: MemoryLayout<Result<String, VoxError<Infallible>>>.alignment)
    defer { raw.deallocate() }
    try decodeInto(testbed_echo_ResponseDecodeProgram, payload, raw)
    let decoded = raw.assumingMemoryBound(to: Result<String, VoxError<Infallible>>.self).move()
    guard case .success(let v) = decoded else {
        Issue.record("expected .success, got \(decoded)")
        return
    }
    #expect(v == "HELLO WORLD")
}

// A fallible method: divide(dividend, divisor) -> Result<Int64, MathError>; args is a
// 2-tuple, response is Result<Int64, VoxError<MathError>>.
@Test func divideArgsRoundTrip() throws {
    var args: (Int64, Int64) = (42, 7)
    let payload = withUnsafeBytes(of: &args) {
        encodeWith(testbed_divide_ArgsEncodeProgram, $0.baseAddress!)
    }
    let raw = UnsafeMutableRawPointer.allocate(
        byteCount: MemoryLayout<(Int64, Int64)>.size,
        alignment: MemoryLayout<(Int64, Int64)>.alignment)
    defer { raw.deallocate() }
    try decodeInto(testbed_divide_ArgsDecodeProgram, payload, raw)
    let decoded = raw.assumingMemoryBound(to: (Int64, Int64).self).move()
    #expect(decoded.0 == 42 && decoded.1 == 7)
}
