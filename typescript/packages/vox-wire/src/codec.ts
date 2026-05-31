// Wire codec for the vox `Message` envelope, on the phon engine.
//
// The envelope is an evolvable wire type like any other: decode reconciles the
// peer's `Message` schema (exchanged in the handshake) against our own via phon's
// compatibility plan (`r[compat.plan-first]`). With no peer schema it degenerates
// to writer==reader — the same plan, not a shortcut.

import {
  type Registry,
  type Schema,
  schemaFromBytes,
  hexToBytes,
} from "@bearcove/phon-schema";
import { type CompiledDecoder, compile, encodeTyped } from "@bearcove/phon-engine";

import type { Message } from "./types.ts";
import { registry, schemaId } from "./wire.phon.generated.ts";

/** Encode a `Message` to phon-compact bytes against our local envelope schema. */
export function encodeMessage(message: Message): Uint8Array {
  return encodeTyped(message as unknown as never, schemaId.Message, registry);
}

/** A reusable compat decode program for the `Message` envelope. */
export type MessageDecoder = CompiledDecoder;

/**
 * Build a decoder for incoming `Message`s. `peerSchemaBytes` is the peer's
 * envelope schema closure (phon self-describing schema bytes) from the handshake;
 * when absent, our own schema is the writer (drift-free degenerate of the one
 * compat path).
 */
export function buildMessageDecoder(peerSchemaBytes?: Uint8Array): MessageDecoder {
  if (!peerSchemaBytes || peerSchemaBytes.length === 0) {
    return compile(schemaId.Message, schemaId.Message, registry);
  }
  const { root, reg } = mergeWriterSchemas(peerSchemaBytes, registry);
  return compile(root, schemaId.Message, reg);
}

/** Decode a `Message` with a prebuilt decoder. */
export function decodeMessageWith(decoder: MessageDecoder, bytes: Uint8Array): Message {
  return decoder(bytes) as unknown as Message;
}

/** Decode a `Message` against our own (same-version) envelope schema. */
export function decodeMessage(bytes: Uint8Array): Message {
  return compile(schemaId.Message, schemaId.Message, registry)(bytes) as unknown as Message;
}

/**
 * Parse a phon schema closure (`u64 root + u32 count + [u32 len + schema]*`,
 * vox-phon's `schema_bytes` framing) and merge its schemas into a registry that
 * also resolves the local refs.
 */
export function parseSchemaClosure(bytes: Uint8Array): { root: bigint; schemas: Schema[] } {
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let off = 0;
  const root = dv.getBigUint64(off, true);
  off += 8;
  const count = dv.getUint32(off, true);
  off += 4;
  const schemas: Schema[] = [];
  for (let i = 0; i < count; i++) {
    const len = dv.getUint32(off, true);
    off += 4;
    const slice = bytes.subarray(off, off + len);
    off += len;
    schemas.push(schemaFromBytes(slice));
  }
  return { root, schemas };
}

function mergeWriterSchemas(
  peerSchemaBytes: Uint8Array,
  local: Registry,
): { root: bigint; reg: Registry } {
  const { root, schemas } = parseSchemaClosure(peerSchemaBytes);
  return { root, reg: local.with(schemas) };
}
