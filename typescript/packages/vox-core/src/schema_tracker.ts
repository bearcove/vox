// Schema exchange on the phon engine.
//
// A peer advertises its type for a (method, direction) binding as a phon
// schema-closure (self-describing bytes) in the `schemas:` field. The receiver
// records the writer closure and builds a compatibility decoder reconciling it
// against the local reader type. Field matching, reordering, and defaulting are
// phon's compatibility plan; vox only records the
// writer closure and asks phon to build the decoder.
//
// r[impl schema.tracking.received]

import { type Registry, type Schema, hexToBytes } from "@bearcove/phon-schema";
import { type Typed, decodeTyped } from "@bearcove/phon-engine";
import { parseSchemaClosure } from "@bearcove/vox-wire";

/** A reusable compat decoder yielding the ergonomic `{ tag, value }` shape. */
export type TypedDecoder = (bytes: Uint8Array) => Typed;

export type BindingDirection = "args" | "response";

/** Per-method schema data emitted by vox-codegen (`{service}Methods`). */
export interface PhonChannelMeta {
  index: number;
  direction: "tx" | "rx";
  elementRoot: bigint;
}
export interface PhonMethodSchemas {
  argsRoot: bigint;
  argsSchemaClosure: string;
  okRoot: bigint;
  /** Root of the response wire type `Result<T, VoxError<E>>` (server encode). */
  responseRoot: bigint;
  /** Schema-closure hex for the response wire type (advertised by the server). */
  responseSchemaClosure: string;
  channels: PhonChannelMeta[];
}

const bindingKey = (methodId: bigint, direction: BindingDirection): string =>
  `${methodId}:${direction}`;

/**
 * Tracks the writer schema closures a peer advertised, and builds compat decoders
 * against local reader roots.
 */
export class SchemaTracker {
  private received = new Map<string, { root: bigint; schemas: Schema[] }>();
  // Cache of built decoders, keyed by (method, direction, readerRoot).
  private decoders = new Map<string, TypedDecoder>();

  reset(): void {
    this.received.clear();
    this.decoders.clear();
  }

  /**
   * Record the peer's phon schema-closure bytes for a binding. Best-effort and
   * idempotent — receiving a schema again simply overwrites (best-effort).
   */
  recordReceived(methodId: bigint, direction: BindingDirection, schemaBytes: Uint8Array): void {
    if (schemaBytes.length === 0) return;
    this.received.set(bindingKey(methodId, direction), parseSchemaClosure(schemaBytes));
    this.decoders.delete(`${bindingKey(methodId, direction)}`);
  }

  hasReceived(methodId: bigint, direction: BindingDirection): boolean {
    return this.received.has(bindingKey(methodId, direction));
  }

  /**
   * Build (and cache) a compat decoder for `(methodId, direction)` producing the
   * reader type identified by `readerRoot`, resolved through `local` plus the
   * writer's exchanged schemas. Returns null when no writer schema was received.
   */
  buildDecoder(
    methodId: bigint,
    direction: BindingDirection,
    readerRoot: bigint,
    local: Registry,
  ): TypedDecoder | null {
    const writer = this.received.get(bindingKey(methodId, direction));
    if (!writer) return null;
    const cacheKey = `${bindingKey(methodId, direction)}:${readerRoot}`;
    const cached = this.decoders.get(cacheKey);
    if (cached) return cached;
    const reg = local.with(writer.schemas);
    const decoder: TypedDecoder = (bytes) => decodeTyped(bytes, writer.root, readerRoot, reg);
    this.decoders.set(cacheKey, decoder);
    return decoder;
  }

  /**
   * Decode against the writer's OWN advertised schema (writer == reader). Used for
   * responses, whose wire type is `Result<T, VoxError<E>>` — the server advertises
   * it and we decode the `{ tag: "Ok" | "Err", value }` structure directly. `local`
   * supplies the primitive table; the writer closure supplies every composite.
   */
  buildWriterDecoder(
    methodId: bigint,
    direction: BindingDirection,
    local: Registry,
  ): TypedDecoder | null {
    const writer = this.received.get(bindingKey(methodId, direction));
    if (!writer) return null;
    const cacheKey = `${bindingKey(methodId, direction)}:writer`;
    const cached = this.decoders.get(cacheKey);
    if (cached) return cached;
    const reg = local.with(writer.schemas);
    const decoder: TypedDecoder = (bytes) => decodeTyped(bytes, writer.root, writer.root, reg);
    this.decoders.set(cacheKey, decoder);
    return decoder;
  }
}

export class SchemaTranslationError extends Error {
  constructor(message: string) {
    super(`Schema translation error: ${message}`);
    this.name = "SchemaTranslationError";
  }
}

/**
 * Tracks which (method, direction) schema closures have been advertised on a
 * connection, so each is sent at most once (`r[schema.exchange.idempotent]`).
 */
export class SchemaSendTracker {
  private sent = new Set<string>();

  reset(): void {
    this.sent.clear();
  }

  /**
   * The phon schema-closure bytes (as a `number[]` for the `schemas:` wire field)
   * to advertise for `(methodId, direction)`, or `[]` when already sent. The
   * closure hex comes from the generated `{service}Methods` table.
   */
  prepareSchemas(methodId: bigint, direction: BindingDirection, closureHex: string): number[] {
    const key = bindingKey(methodId, direction);
    if (this.sent.has(key)) return [];
    this.sent.add(key);
    return Array.from(hexToBytes(closureHex));
  }
}
