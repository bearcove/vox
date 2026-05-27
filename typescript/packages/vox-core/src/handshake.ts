import type { Schema } from "@bearcove/binette";
import { decodeWithTypeRef, encodeWithTypeRef } from "@bearcove/binette";
import type { ConnectionSettings, Parity, MetadataEntry, Metadata } from "@bearcove/vox-wire";
import { handshakeMessageRootRef, handshakeMessageSchemaRegistry, messageSchemas } from "@bearcove/vox-wire";
import type { Link } from "./link.ts";
import { normalizeSchemaList, schemaListFromRust, schemaListToRust } from "./schema_binette.ts";

export type { MetadataEntry, Metadata };

export interface HandshakeResult {
  localSettings: ConnectionSettings;
  peerSettings: ConnectionSettings;
  peerSupportsRetry: boolean;
  sessionResumeKey: Uint8Array | null;
  peerResumeKey: Uint8Array | null;
  peerMessageSchema: Schema[];
  peerMetadata: MetadataEntry[];
}

type HandshakeMessage =
  | { tag: "Hello"; value: HelloMessage }
  | { tag: "HelloYourself"; value: HelloYourselfMessage }
  | { tag: "LetsGo" }
  | { tag: "Sorry"; reason: string };

interface HelloMessage {
  parity: Parity;
  connection_settings: ConnectionSettings;
  message_payload_schema: Schema[];
  supports_retry: boolean;
  resume_key: Uint8Array | null;
  metadata: Metadata;
}

interface HelloYourselfMessage {
  connection_settings: ConnectionSettings;
  message_payload_schema: Schema[];
  supports_retry: boolean;
  resume_key: Uint8Array | null;
  metadata: Metadata;
}

type RustSchema = ReturnType<typeof schemaListToRust>[number];

type WireResumeKey = { bytes: Uint8Array };

type WireHelloMessage = Omit<HelloMessage, "message_payload_schema" | "resume_key"> & {
  message_payload_schema: RustSchema[];
  resume_key: WireResumeKey | null;
};

type WireHelloYourselfMessage = Omit<HelloYourselfMessage, "message_payload_schema" | "resume_key"> & {
  message_payload_schema: RustSchema[];
  resume_key: WireResumeKey | null;
};

type WireHandshakeMessage =
  | { tag: "Hello"; value: WireHelloMessage }
  | { tag: "HelloYourself"; value: WireHelloYourselfMessage }
  | { tag: "LetsGo"; value: Record<string, never> }
  | { tag: "Sorry"; value: { reason: string } };

function resumeKeyToWire(resumeKey: Uint8Array | null): WireResumeKey | null {
  return resumeKey === null ? null : { bytes: resumeKey };
}

function resumeKeyFromWire(resumeKey: WireResumeKey | null): Uint8Array | null {
  return resumeKey === null ? null : resumeKey.bytes.slice();
}

function messageToWire(message: HandshakeMessage): WireHandshakeMessage {
  switch (message.tag) {
    case "Hello":
      return {
        tag: "Hello",
        value: {
          ...message.value,
          message_payload_schema: schemaListToRust(message.value.message_payload_schema),
          resume_key: resumeKeyToWire(message.value.resume_key),
        },
      };
    case "HelloYourself":
      return {
        tag: "HelloYourself",
        value: {
          ...message.value,
          message_payload_schema: schemaListToRust(message.value.message_payload_schema),
          resume_key: resumeKeyToWire(message.value.resume_key),
        },
      };
    case "LetsGo":
      return { tag: "LetsGo", value: {} };
    case "Sorry":
      return { tag: "Sorry", value: { reason: message.reason } };
  }
}

function messageFromWire(message: WireHandshakeMessage): HandshakeMessage {
  switch (message.tag) {
    case "Hello":
      return {
        tag: "Hello",
        value: {
          ...message.value,
          message_payload_schema: schemaListFromRust(message.value.message_payload_schema),
          resume_key: resumeKeyFromWire(message.value.resume_key),
        },
      };
    case "HelloYourself":
      return {
        tag: "HelloYourself",
        value: {
          ...message.value,
          message_payload_schema: schemaListFromRust(message.value.message_payload_schema),
          resume_key: resumeKeyFromWire(message.value.resume_key),
        },
      };
    case "LetsGo":
      return { tag: "LetsGo" };
    case "Sorry":
      return { tag: "Sorry", reason: message.value.reason };
  }
}

function encodeHandshakeMessage(message: HandshakeMessage): Uint8Array {
  return encodeWithTypeRef(messageToWire(message), handshakeMessageRootRef, handshakeMessageSchemaRegistry);
}

function parseHandshakeMessage(bytes: Uint8Array): HandshakeMessage {
  const decoded = decodeWithTypeRef(bytes, 0, handshakeMessageRootRef, handshakeMessageSchemaRegistry);
  if (decoded.next !== bytes.length) {
    throw new Error(`handshake: trailing ${bytes.length - decoded.next} bytes`);
  }
  return messageFromWire(decoded.value as WireHandshakeMessage);
}

async function recvHandshake(link: Link): Promise<HandshakeMessage> {
  const payload = await link.recv();
  if (!payload) {
    throw new Error("peer closed during handshake");
  }
  return parseHandshakeMessage(payload);
}

async function sendHandshake(link: Link, message: HandshakeMessage): Promise<void> {
  await link.send(encodeHandshakeMessage(message));
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
  supportsRetry: boolean = true,
  resumeKey: Uint8Array | null = null,
  metadata: Metadata = [],
): Promise<HandshakeResult> {
  await sendHandshake(link, {
    tag: "Hello",
    value: {
      parity: settings.parity,
      connection_settings: settings,
      message_payload_schema: messageSchemas,
      supports_retry: supportsRetry,
      resume_key: resumeKey,
      metadata,
    },
  });

  const response = await recvHandshake(link);
  if (response.tag === "Sorry") {
    throw new Error(`handshake rejected: ${response.reason}`);
  }
  if (response.tag !== "HelloYourself") {
    throw new Error("expected HelloYourself during handshake");
  }

  await sendHandshake(link, { tag: "LetsGo" });

  return {
    localSettings: settings,
    peerSettings: response.value.connection_settings,
    peerSupportsRetry: response.value.supports_retry,
    sessionResumeKey: response.value.resume_key,
    peerResumeKey: null,
    peerMessageSchema: normalizeSchemaList(response.value.message_payload_schema),
    peerMetadata: response.value.metadata,
  };
}

export async function handshakeAsAcceptor(
  link: Link,
  settings: ConnectionSettings,
  supportsRetry: boolean = true,
  resumable: boolean = false,
  expectedResumeKey: Uint8Array | null = null,
  metadata: Metadata = [],
): Promise<HandshakeResult> {
  const first = await recvHandshake(link);
  if (first.tag !== "Hello") {
    throw new Error("expected Hello during handshake");
  }

  if (expectedResumeKey) {
    const actual = first.value.resume_key;
    if (!actual || !sameBytes(actual, expectedResumeKey)) {
      await sendHandshake(link, {
        tag: "Sorry",
        reason: "session resume key mismatch",
      });
      throw new Error("session resume key mismatch");
    }
  }

  const sessionResumeKey = resumable ? randomSessionResumeKey() : null;
  await sendHandshake(link, {
    tag: "HelloYourself",
    value: {
      connection_settings: settings,
      message_payload_schema: messageSchemas,
      supports_retry: supportsRetry,
      resume_key: sessionResumeKey,
      metadata,
    },
  });

  const third = await recvHandshake(link);
  if (third.tag === "Sorry") {
    throw new Error(`handshake rejected: ${third.reason}`);
  }
  if (third.tag !== "LetsGo") {
    throw new Error("expected LetsGo during handshake");
  }

  return {
    localSettings: settings,
    peerSettings: first.value.connection_settings,
    peerSupportsRetry: first.value.supports_retry,
    sessionResumeKey,
    peerResumeKey: first.value.resume_key,
    peerMessageSchema: normalizeSchemaList(first.value.message_payload_schema),
    peerMetadata: first.value.metadata,
  };
}

export function voxServiceMetadata(serviceName: string): Metadata {
  return [{ key: "vox-service", value: { tag: "String", value: serviceName }, flags: 0n }];
}
