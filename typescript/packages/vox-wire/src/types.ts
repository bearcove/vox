// Vox wire protocol types — the Message envelope and its payloads.
//
// The envelope types + the phon `registry`/`schemaId` are generated from the Rust
// `Message` shape into `wire.phon.generated.ts`; this module re-exports them and
// adds the hand-written metadata model + message constructors.

import type { Value } from "@bearcove/phon-schema";

export type {
  Message,
  MessagePayload,
  ProtocolError,
  ConnectionOpen,
  ConnectionAccept,
  ConnectionReject,
  ConnectionClose,
  RequestMessage,
  RequestBody,
  RequestCall,
  RequestResponse,
  RequestCancel,
  SchemaMessage,
  BindingDirection,
  ChannelMessage,
  ChannelBody,
  ChannelItem,
  ChannelClose,
  ChannelReset,
  ChannelGrantCredit,
  ConnectionSettings,
  Parity,
  Ping,
  Pong,
} from "./wire.phon.generated.ts";

import type { ConnectionSettings, Message, Parity } from "./wire.phon.generated.ts";

// Branded id aliases (all `bigint` on the wire).
export type ConnectionId = bigint;
export type RequestId = bigint;
export type MethodId = bigint;
export type ChannelId = bigint;

// ---------------------------------------------------------------------------
// Metadata
//
// Metadata is a self-describing `Value` map (`r[rpc.metadata]`): keys are
// strings, values are phon `Value`s. Flags became well-known keys whose value is
// a list of the key names they apply to (`r[rpc.metadata.flags]`).
// ---------------------------------------------------------------------------

export type Metadata = Map<string, Value>;

/** Well-known metadata flag keys. */
export const MetadataKeys = {
  SENSITIVE: "vox:sensitive",
  NO_PROPAGATE: "vox:no-propagate",
} as const;

export function emptyMetadata(): Metadata {
  return new Map();
}

/** Mark `key` under a well-known flag list (`vox:sensitive` / `vox:no-propagate`). */
export function metadataAddFlag(meta: Metadata, flagKey: string, key: string): void {
  const existing = meta.get(flagKey);
  const list: Value[] = Array.isArray(existing) ? (existing as Value[]) : [];
  if (!list.includes(key)) list.push(key);
  meta.set(flagKey, list);
}

export function metadataIsSensitive(meta: Metadata, key: string): boolean {
  const list = meta.get(MetadataKeys.SENSITIVE);
  return Array.isArray(list) && (list as Value[]).includes(key);
}

export function metadataIsNoPropagate(meta: Metadata, key: string): boolean {
  const list = meta.get(MetadataKeys.NO_PROPAGATE);
  return Array.isArray(list) && (list as Value[]).includes(key);
}

// ---------------------------------------------------------------------------
// Message constructors
// ---------------------------------------------------------------------------

export function parityOdd(): Parity {
  return { tag: "Odd" };
}

export function parityEven(): Parity {
  return { tag: "Even" };
}

export function connectionSettings(
  parity: Parity,
  maxConcurrentRequests: number,
  initialChannelCredit = 16,
): ConnectionSettings {
  return {
    parity,
    max_concurrent_requests: maxConcurrentRequests,
    initial_channel_credit: initialChannelCredit,
  };
}

export function messageProtocolError(description: string, connId: bigint = 0n): Message {
  return { connection_id: connId, payload: { tag: "ProtocolError", value: { description } } };
}

export function messagePing(nonce: bigint, connId: bigint = 0n): Message {
  return { connection_id: connId, payload: { tag: "Ping", value: { nonce } } };
}

export function messagePong(nonce: bigint, connId: bigint = 0n): Message {
  return { connection_id: connId, payload: { tag: "Pong", value: { nonce } } };
}

export function messageConnect(
  connId: bigint,
  connection_settings: ConnectionSettings,
  metadata: Metadata = emptyMetadata(),
): Message {
  return {
    connection_id: connId,
    payload: { tag: "ConnectionOpen", value: { connection_settings, metadata } },
  };
}

export function messageAccept(
  connId: bigint,
  connection_settings: ConnectionSettings,
  metadata: Metadata = emptyMetadata(),
): Message {
  return {
    connection_id: connId,
    payload: { tag: "ConnectionAccept", value: { connection_settings, metadata } },
  };
}

export function messageReject(connId: bigint, metadata: Metadata = emptyMetadata()): Message {
  return { connection_id: connId, payload: { tag: "ConnectionReject", value: { metadata } } };
}

export function messageGoodbye(connId: bigint = 0n, metadata: Metadata = emptyMetadata()): Message {
  return { connection_id: connId, payload: { tag: "ConnectionClose", value: { metadata } } };
}

export function messageRequest(
  requestId: bigint,
  methodId: bigint,
  payload: Uint8Array,
  metadata: Metadata = emptyMetadata(),
  channels: bigint[] = [],
  connId: bigint = 0n,
  schemas: number[] = [],
): Message {
  return {
    connection_id: connId,
    payload: {
      tag: "RequestMessage",
      value: {
        id: requestId,
        body: {
          tag: "Call",
          value: { method_id: methodId, channels, metadata, args: payload, schemas },
        },
      },
    },
  };
}

export function messageResponse(
  requestId: bigint,
  payload: Uint8Array,
  metadata: Metadata = emptyMetadata(),
  connId: bigint = 0n,
  schemas: number[] = [],
): Message {
  return {
    connection_id: connId,
    payload: {
      tag: "RequestMessage",
      value: {
        id: requestId,
        body: { tag: "Response", value: { ret: payload, metadata, schemas } },
      },
    },
  };
}

export function messageCancel(
  requestId: bigint,
  connId: bigint = 0n,
  metadata: Metadata = emptyMetadata(),
): Message {
  return {
    connection_id: connId,
    payload: {
      tag: "RequestMessage",
      value: { id: requestId, body: { tag: "Cancel", value: { metadata } } },
    },
  };
}

export function messageData(channelId: bigint, payload: Uint8Array, connId: bigint = 0n): Message {
  return {
    connection_id: connId,
    payload: {
      tag: "ChannelMessage",
      value: { id: channelId, body: { tag: "Item", value: { item: payload } } },
    },
  };
}

export function messageClose(
  channelId: bigint,
  connId: bigint = 0n,
  metadata: Metadata = emptyMetadata(),
): Message {
  return {
    connection_id: connId,
    payload: {
      tag: "ChannelMessage",
      value: { id: channelId, body: { tag: "Close", value: { metadata } } },
    },
  };
}

export function messageReset(
  channelId: bigint,
  connId: bigint = 0n,
  metadata: Metadata = emptyMetadata(),
): Message {
  return {
    connection_id: connId,
    payload: {
      tag: "ChannelMessage",
      value: { id: channelId, body: { tag: "Reset", value: { metadata } } },
    },
  };
}

export function messageCredit(channelId: bigint, additional: number, connId: bigint = 0n): Message {
  return {
    connection_id: connId,
    payload: {
      tag: "ChannelMessage",
      value: { id: channelId, body: { tag: "GrantCredit", value: { additional } } },
    },
  };
}
