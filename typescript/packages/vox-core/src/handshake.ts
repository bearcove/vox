import { hexToBytes } from "@bearcove/phon-schema";
import { decodeTyped, encodeTyped } from "@bearcove/phon-engine";
import { parseSchemaClosure, messageSchemaClosure, type Metadata, emptyMetadata, coerceMetadata } from "@bearcove/vox-wire";
import type { ConnectionSettings, Parity } from "@bearcove/vox-wire";
import type { Link } from "./link.ts";
import {
  registry,
  schemaId,
  handshakeSchemaClosure,
  type HandshakeMessage,
} from "./handshake.phon.generated.ts";

// Re-export Metadata for downstream consumers that used to import it from here.
export type { Metadata } from "@bearcove/vox-wire";

export interface HandshakeResult {
  localSettings: ConnectionSettings;
  peerSettings: ConnectionSettings;
  peerMessageSchema: Uint8Array;
  peerMetadata: Metadata;
}

// ---------------------------------------------------------------------------
// phon self-describing framing
//
// Each handshake message is sent as:
//   [u32 schema_len little-endian][schema-closure bytes][phon-compact value]
// ---------------------------------------------------------------------------

function encodeHandshake(msg: HandshakeMessage): Uint8Array {
  const value = encodeTyped(msg as never, schemaId.HandshakeMessage, registry);
  const closure = hexToBytes(handshakeSchemaClosure);

  const out = new Uint8Array(4 + closure.length + value.length);
  const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
  dv.setUint32(0, closure.length, true);
  out.set(closure, 4);
  out.set(value, 4 + closure.length);
  return out;
}

function decodeHandshake(bytes: Uint8Array): HandshakeMessage {
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const len = dv.getUint32(0, true);
  const closure = bytes.subarray(4, 4 + len);
  const value = bytes.subarray(4 + len);
  const { root, schemas } = parseSchemaClosure(closure);
  return decodeTyped(
    value,
    root,
    schemaId.HandshakeMessage,
    registry.with(schemas),
  ) as unknown as HandshakeMessage;
}

async function recvHandshake(link: Link): Promise<HandshakeMessage> {
  const payload = await link.recv();
  if (!payload) {
    throw new Error("peer closed during handshake");
  }
  return decodeHandshake(payload);
}

async function sendHandshake(link: Link, message: HandshakeMessage): Promise<void> {
  await link.send(encodeHandshake(message));
}

// The sender's Message-envelope schema closure, sent verbatim as a byte list.
function localMessagePayloadSchema(): number[] {
  return Array.from(hexToBytes(messageSchemaClosure));
}

export async function handshakeAsInitiator(
  link: Link,
  settings: ConnectionSettings,
  metadata: Metadata = emptyMetadata(),
): Promise<HandshakeResult> {
  await sendHandshake(link, {
    tag: "Hello",
    value: {
      parity: settings.parity,
      connection_settings: settings,
      message_payload_schema: localMessagePayloadSchema(),
      metadata,
    },
  });

  const response = await recvHandshake(link);
  if (response.tag === "Sorry") {
    throw new Error(`handshake rejected: ${response.value.reason}`);
  }
  if (response.tag !== "HelloYourself") {
    throw new Error("expected HelloYourself during handshake");
  }

  await sendHandshake(link, { tag: "LetsGo", value: {} });

  const helloYourself = response;
  const peerMetadata = coerceMetadata(helloYourself.value.metadata);
  return {
    localSettings: settings,
    peerSettings: helloYourself.value.connection_settings,
    peerMessageSchema: new Uint8Array(helloYourself.value.message_payload_schema),
    peerMetadata,
  };
}

export async function handshakeAsAcceptor(
  link: Link,
  settings: ConnectionSettings,
  metadata: Metadata = emptyMetadata(),
): Promise<HandshakeResult> {
  const first = await recvHandshake(link);
  if (first.tag !== "Hello") {
    throw new Error("expected Hello during handshake");
  }
  const hello = first;

  await sendHandshake(link, {
    tag: "HelloYourself",
    value: {
      connection_settings: settings,
      message_payload_schema: localMessagePayloadSchema(),
      metadata,
    },
  });

  const third = await recvHandshake(link);
  if (third.tag === "Sorry") {
    throw new Error(`handshake rejected: ${third.value.reason}`);
  }
  if (third.tag !== "LetsGo") {
    throw new Error("expected LetsGo during handshake");
  }

  const peerMetadata = coerceMetadata(hello.value.metadata);
  return {
    localSettings: settings,
    peerSettings: hello.value.connection_settings,
    peerMessageSchema: new Uint8Array(hello.value.message_payload_schema),
    peerMetadata,
  };
}

export function voxServiceMetadata(serviceName: string): Metadata {
  const metadata: Metadata = new Map();
  metadata.set("vox-service", serviceName);
  return metadata;
}
