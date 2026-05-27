export function decodeBytes(buf: Uint8Array, offset: number): { value: Uint8Array; next: number } {
  const len = readU32(buf, offset, "bytes");
  const start = offset + 4;
  const end = start + len;
  if (end > buf.length) throw new Error("bytes: overrun");
  return { value: buf.subarray(start, end), next: end };
}

function readU32(buf: Uint8Array, offset: number, context: string): number {
  if (offset + 4 > buf.length) throw new Error(`${context}: eof`);
  return new DataView(buf.buffer, buf.byteOffset + offset, 4).getUint32(0, true);
}
