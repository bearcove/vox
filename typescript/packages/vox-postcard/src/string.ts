export function decodeString(buf: Uint8Array, offset: number): { value: string; next: number } {
  const len = readU32(buf, offset, "string");
  const start = offset + 4;
  const end = start + len;
  if (end > buf.length) throw new Error("string: overrun");
  const s = new TextDecoder().decode(buf.subarray(start, end));
  return { value: s, next: end };
}

function readU32(buf: Uint8Array, offset: number, context: string): number {
  if (offset + 4 > buf.length) throw new Error(`${context}: eof`);
  return new DataView(buf.buffer, buf.byteOffset + offset, 4).getUint32(0, true);
}
