// binette compact serialization helpers for TypeScript.

import { concat } from "./binary/bytes.ts";

// ============================================================================
// Decode result type
// ============================================================================

export interface DecodeResult<T> {
  value: T;
  next: number; // offset after this value
}

function fixed(width: number, write: (view: DataView) => void): Uint8Array {
  const buf = new ArrayBuffer(width);
  write(new DataView(buf));
  return new Uint8Array(buf);
}

function viewFor(buf: Uint8Array, offset: number, width: number, context: string): DataView {
  if (offset + width > buf.length) throw new Error(`${context}: eof`);
  return new DataView(buf.buffer, buf.byteOffset + offset, width);
}

// ============================================================================
// Primitive encoding
// ============================================================================

/** Encode a boolean (1 byte: 0x00 or 0x01). */
export function encodeBool(value: boolean): Uint8Array {
  return Uint8Array.of(value ? 1 : 0);
}

/** Decode a boolean. */
export function decodeBool(buf: Uint8Array, offset: number): DecodeResult<boolean> {
  if (offset >= buf.length) throw new Error("bool: eof");
  const byte = buf[offset];
  if (byte > 1) throw new Error(`bool: invalid value ${byte}`);
  return { value: byte === 1, next: offset + 1 };
}

/** Encode a u8 (1 byte). */
export function encodeU8(value: number): Uint8Array {
  return Uint8Array.of(value & 0xff);
}

/** Decode a u8. */
export function decodeU8(buf: Uint8Array, offset: number): DecodeResult<number> {
  if (offset >= buf.length) throw new Error("u8: eof");
  return { value: buf[offset], next: offset + 1 };
}

/** Encode an i8 (1 byte, two's complement). */
export function encodeI8(value: number): Uint8Array {
  return Uint8Array.of(value & 0xff);
}

/** Decode an i8. */
export function decodeI8(buf: Uint8Array, offset: number): DecodeResult<number> {
  if (offset >= buf.length) throw new Error("i8: eof");
  const byte = buf[offset];
  // Convert to signed
  const value = byte > 127 ? byte - 256 : byte;
  return { value, next: offset + 1 };
}

/** Encode a u16 (2 little-endian bytes). */
export function encodeU16(value: number): Uint8Array {
  return fixed(2, (view) => view.setUint16(0, value, true));
}

/** Decode a u16. */
export function decodeU16(buf: Uint8Array, offset: number): DecodeResult<number> {
  return { value: viewFor(buf, offset, 2, "u16").getUint16(0, true), next: offset + 2 };
}

/** Encode a u32 (4 little-endian bytes). */
export function encodeU32(value: number): Uint8Array {
  return fixed(4, (view) => view.setUint32(0, value, true));
}

/** Decode a u32. */
export function decodeU32(buf: Uint8Array, offset: number): DecodeResult<number> {
  return { value: viewFor(buf, offset, 4, "u32").getUint32(0, true), next: offset + 4 };
}

/** Encode a u64 (8 little-endian bytes). */
export function encodeU64(value: bigint): Uint8Array {
  return fixed(8, (view) => view.setBigUint64(0, value, true));
}

/** Decode a u64. */
export function decodeU64(buf: Uint8Array, offset: number): DecodeResult<bigint> {
  return { value: viewFor(buf, offset, 8, "u64").getBigUint64(0, true), next: offset + 8 };
}

/** Encode a u128 (16 little-endian bytes). */
export function encodeU128(value: bigint): Uint8Array {
  return fixed(16, (view) => {
    view.setBigUint64(0, value & 0xffff_ffff_ffff_ffffn, true);
    view.setBigUint64(8, value >> 64n, true);
  });
}

/** Decode a u128. */
export function decodeU128(buf: Uint8Array, offset: number): DecodeResult<bigint> {
  const view = viewFor(buf, offset, 16, "u128");
  const lo = view.getBigUint64(0, true);
  const hi = view.getBigUint64(8, true);
  return { value: lo | (hi << 64n), next: offset + 16 };
}

/** Encode an i16 (2 little-endian bytes). */
export function encodeI16(value: number): Uint8Array {
  return fixed(2, (view) => view.setInt16(0, value, true));
}

/** Decode an i16. */
export function decodeI16(buf: Uint8Array, offset: number): DecodeResult<number> {
  return { value: viewFor(buf, offset, 2, "i16").getInt16(0, true), next: offset + 2 };
}

/** Encode an i32 (4 little-endian bytes). */
export function encodeI32(value: number): Uint8Array {
  return fixed(4, (view) => view.setInt32(0, value, true));
}

/** Decode an i32. */
export function decodeI32(buf: Uint8Array, offset: number): DecodeResult<number> {
  return { value: viewFor(buf, offset, 4, "i32").getInt32(0, true), next: offset + 4 };
}

/** Encode an i64 (8 little-endian bytes). */
export function encodeI64(value: bigint): Uint8Array {
  return fixed(8, (view) => view.setBigInt64(0, value, true));
}

/** Decode an i64. */
export function decodeI64(buf: Uint8Array, offset: number): DecodeResult<bigint> {
  return { value: viewFor(buf, offset, 8, "i64").getBigInt64(0, true), next: offset + 8 };
}

/** Encode an i128 (16 little-endian two's-complement bytes). */
export function encodeI128(value: bigint): Uint8Array {
  const unsigned = BigInt.asUintN(128, value);
  return fixed(16, (view) => {
    view.setBigUint64(0, unsigned & 0xffff_ffff_ffff_ffffn, true);
    view.setBigUint64(8, unsigned >> 64n, true);
  });
}

/** Decode an i128. */
export function decodeI128(buf: Uint8Array, offset: number): DecodeResult<bigint> {
  const decoded = decodeU128(buf, offset);
  return { value: BigInt.asIntN(128, decoded.value), next: decoded.next };
}

/** Encode an f32 (4 bytes little-endian IEEE 754). */
export function encodeF32(value: number): Uint8Array {
  const buf = new ArrayBuffer(4);
  new DataView(buf).setFloat32(0, value, true);
  return new Uint8Array(buf);
}

/** Decode an f32. */
export function decodeF32(buf: Uint8Array, offset: number): DecodeResult<number> {
  if (offset + 4 > buf.length) throw new Error("f32: eof");
  const view = new DataView(buf.buffer, buf.byteOffset + offset, 4);
  return { value: view.getFloat32(0, true), next: offset + 4 };
}

/** Encode an f64 (8 bytes little-endian IEEE 754). */
export function encodeF64(value: number): Uint8Array {
  const buf = new ArrayBuffer(8);
  new DataView(buf).setFloat64(0, value, true);
  return new Uint8Array(buf);
}

/** Decode an f64. */
export function decodeF64(buf: Uint8Array, offset: number): DecodeResult<number> {
  if (offset + 8 > buf.length) throw new Error("f64: eof");
  const view = new DataView(buf.buffer, buf.byteOffset + offset, 8);
  return { value: view.getFloat64(0, true), next: offset + 8 };
}

// ============================================================================
// String encoding
// ============================================================================

/** Encode a string (length-prefixed UTF-8). */
export function encodeString(value: string): Uint8Array {
  const bytes = new TextEncoder().encode(value);
  return concat(encodeU32(bytes.length), bytes);
}

/** Decode a string. */
export function decodeString(buf: Uint8Array, offset: number): DecodeResult<string> {
  const len = decodeU32(buf, offset);
  const start = len.next;
  const end = start + len.value;
  if (end > buf.length) throw new Error("string: overrun");
  const s = new TextDecoder().decode(buf.subarray(start, end));
  return { value: s, next: end };
}

// ============================================================================
// Bytes encoding
// ============================================================================

/** Encode bytes (length-prefixed). */
export function encodeBytes(value: Uint8Array): Uint8Array {
  return concat(encodeU32(value.length), value);
}

/** Decode bytes. */
export function decodeBytes(buf: Uint8Array, offset: number): DecodeResult<Uint8Array> {
  const len = decodeU32(buf, offset);
  const start = len.next;
  const end = start + len.value;
  if (end > buf.length) throw new Error("bytes: overrun");
  return { value: buf.subarray(start, end), next: end };
}

// ============================================================================
// Option encoding
// ============================================================================

/** Encode an Option<T>. */
export function encodeOption<T>(value: T | null, encodeInner: (v: T) => Uint8Array): Uint8Array {
  if (value === null) {
    return Uint8Array.of(0);
  } else {
    return concat(Uint8Array.of(1), encodeInner(value));
  }
}

/** Decode an Option<T>. */
export function decodeOption<T>(
  buf: Uint8Array,
  offset: number,
  decodeInner: (buf: Uint8Array, offset: number) => DecodeResult<T>,
): DecodeResult<T | null> {
  if (offset >= buf.length) throw new Error("option: eof");
  const variant = buf[offset];
  if (variant === 0) {
    return { value: null, next: offset + 1 };
  } else if (variant === 1) {
    const inner = decodeInner(buf, offset + 1);
    return { value: inner.value, next: inner.next };
  } else {
    throw new Error(`option: invalid variant ${variant}`);
  }
}

// ============================================================================
// Vec encoding
// ============================================================================

/** Encode a Vec<T>. */
export function encodeVec<T>(values: T[], encodeItem: (v: T) => Uint8Array): Uint8Array {
  const parts: Uint8Array[] = [encodeU32(values.length)];
  for (const item of values) {
    parts.push(encodeItem(item));
  }
  return concat(...parts);
}

/** Decode a Vec<T>. */
export function decodeVec<T>(
  buf: Uint8Array,
  offset: number,
  decodeItem: (buf: Uint8Array, offset: number) => DecodeResult<T>,
): DecodeResult<T[]> {
  const len = decodeU32(buf, offset);
  let pos = len.next;
  const items: T[] = [];
  for (let i = 0; i < len.value; i++) {
    const item = decodeItem(buf, pos);
    items.push(item.value);
    pos = item.next;
  }
  return { value: items, next: pos };
}

// ============================================================================
// Tuple encoding (encode/decode each element in sequence)
// ============================================================================

/** Encode a 2-tuple. */
export function encodeTuple2<A, B>(
  a: A,
  b: B,
  encodeA: (v: A) => Uint8Array,
  encodeB: (v: B) => Uint8Array,
): Uint8Array {
  return concat(encodeA(a), encodeB(b));
}

/** Decode a 2-tuple. */
export function decodeTuple2<A, B>(
  buf: Uint8Array,
  offset: number,
  decodeA: (buf: Uint8Array, offset: number) => DecodeResult<A>,
  decodeB: (buf: Uint8Array, offset: number) => DecodeResult<B>,
): DecodeResult<[A, B]> {
  const a = decodeA(buf, offset);
  const b = decodeB(buf, a.next);
  return { value: [a.value, b.value], next: b.next };
}

/** Encode a 3-tuple. */
export function encodeTuple3<A, B, C>(
  a: A,
  b: B,
  c: C,
  encodeA: (v: A) => Uint8Array,
  encodeB: (v: B) => Uint8Array,
  encodeC: (v: C) => Uint8Array,
): Uint8Array {
  return concat(encodeA(a), encodeB(b), encodeC(c));
}

/** Decode a 3-tuple. */
export function decodeTuple3<A, B, C>(
  buf: Uint8Array,
  offset: number,
  decodeA: (buf: Uint8Array, offset: number) => DecodeResult<A>,
  decodeB: (buf: Uint8Array, offset: number) => DecodeResult<B>,
  decodeC: (buf: Uint8Array, offset: number) => DecodeResult<C>,
): DecodeResult<[A, B, C]> {
  const a = decodeA(buf, offset);
  const b = decodeB(buf, a.next);
  const c = decodeC(buf, b.next);
  return { value: [a.value, b.value, c.value], next: c.next };
}

// ============================================================================
// Struct encoding (encode/decode fields in order)
// ============================================================================

// Structs are encoded by encoding each field in declaration order.
// No special framing - just concatenate the encoded fields.

// ============================================================================
// Enum encoding (variant index + payload)
// ============================================================================

/** Encode an enum variant index. */
export function encodeEnumVariant(variantIndex: number): Uint8Array {
  return encodeU32(variantIndex);
}

/** Decode an enum variant index. */
export function decodeEnumVariant(buf: Uint8Array, offset: number): DecodeResult<number> {
  return decodeU32(buf, offset);
}

// ============================================================================
// Re-export for convenience
// ============================================================================

export { concat };

// ============================================================================
// Canonical schema types
// ============================================================================

export type {
  SchemaHash,
  TypeRef,
  Schema,
  SchemaKind,
  PrimitiveType,
  ChannelDirection,
  FieldSchema,
  VariantSchema,
  VariantPayload,
  SchemaRegistry,
  SchemaPayload,
  BindingDirection,
} from "./schema.ts";

export { resolveTypeRef } from "./schema.ts";

// ============================================================================
// Translation plan
// ============================================================================

export type { TranslationPlan, FieldOp, SchemaSet } from "./plan.ts";
export { buildPlan, schemaSetFromSchemas, TranslationError, IDENTITY } from "./plan.ts";

// ============================================================================
// Plan-driven wire codec
// ============================================================================

export {
  encodeWithTypeRef,
  encodeWithKind,
  decodeWithTypeRef,
  decodeWithKind,
  skipValue,
  decodeWithPlan,
} from "./wire_codec.ts";

export { type VoxErrorPayload } from "./result.ts";
