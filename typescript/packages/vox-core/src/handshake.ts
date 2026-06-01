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
  type ResumeKeyBytes,
} from "./handshake.phon.generated.ts";

// Re-export Metadata for downstream consumers that used to import it from here.
export type { Metadata } from "@bearcove/vox-wire";

export interface HandshakeResult {
  localSettings: ConnectionSettings;
  peerSettings: ConnectionSettings;
  peerSupportsRetry: boolean;
  sessionResumeKey: Uint8Array | null;
  peerResumeKey: Uint8Array | null;
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

function resumeKeyToBytes(key: Uint8Array | null): ResumeKeyBytes | null {
  if (key === null) {
    return null;
  }
  return { bytes: Array.from(key) };
}

function resumeKeyFromBytes(key: ResumeKeyBytes | null): Uint8Array | null {
  if (key === null) {
    return null;
  }
  return new Uint8Array(key.bytes);
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) {
    return false;
  }
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) {
      return false;
    }
  }
  return true;
}

function randomSessionResumeKey(): Uint8Array {
  const bytes = new Uint8Array(16);
  const cryptoApi = globalThis.crypto;
  if (!cryptoApi) {
    throw new Error("crypto.getRandomValues is unavailable");
  }
  cryptoApi.getRandomValues(bytes);
  return bytes;
}

export async function handshakeAsInitiator(
  link: Link,
  settings: ConnectionSettings,
  _supportsRetry: boolean = true,
  resumeKey: Uint8Array | null = null,
  metadata: Metadata = emptyMetadata(),
): Promise<HandshakeResult> {
  await sendHandshake(link, {
    tag: "Hello",
    value: {
      parity: settings.parity,
      connection_settings: settings,
      message_payload_schema: localMessagePayloadSchema(),
      resume_key: resumeKeyToBytes(resumeKey),
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
  return {
    localSettings: settings,
    peerSettings: helloYourself.value.connection_settings,
    // TODO(phon): retry-support advertisement
    peerSupportsRetry: false,
    sessionResumeKey: resumeKeyFromBytes(helloYourself.value.resume_key),
    peerResumeKey: null,
    peerMessageSchema: new Uint8Array(helloYourself.value.message_payload_schema),
    peerMetadata: coerceMetadata(helloYourself.value.metadata),
  };
}

export async function handshakeAsAcceptor(
  link: Link,
  settings: ConnectionSettings,
  _supportsRetry: boolean = true,
  resumable: boolean = false,
  expectedResumeKey: Uint8Array | null = null,
  metadata: Metadata = emptyMetadata(),
): Promise<HandshakeResult> {
  const first = await recvHandshake(link);
  if (first.tag !== "Hello") {
    throw new Error("expected Hello during handshake");
  }
  const hello = first;

  if (expectedResumeKey) {
    const actual = resumeKeyFromBytes(hello.value.resume_key);
    if (!actual || !sameBytes(actual, expectedResumeKey)) {
      await sendHandshake(link, {
        tag: "Sorry",
        value: { reason: "session resume key mismatch" },
      });
      throw new Error("session resume key mismatch");
    }
  }

  const sessionResumeKey = resumable ? randomSessionResumeKey() : null;
  await sendHandshake(link, {
    tag: "HelloYourself",
    value: {
      connection_settings: settings,
      message_payload_schema: localMessagePayloadSchema(),
      resume_key: resumeKeyToBytes(sessionResumeKey),
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

  return {
    localSettings: settings,
    peerSettings: hello.value.connection_settings,
    // TODO(phon): retry-support advertisement
    peerSupportsRetry: false,
    sessionResumeKey,
    peerResumeKey: resumeKeyFromBytes(hello.value.resume_key),
    peerMessageSchema: new Uint8Array(hello.value.message_payload_schema),
    peerMetadata: coerceMetadata(hello.value.metadata),
  };
}

export function voxServiceMetadata(serviceName: string): Metadata {
  const metadata: Metadata = new Map();
  metadata.set("vox-service", serviceName);
  return metadata;
}
