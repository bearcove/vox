// Client-side metadata: a self-describing `Value` map (`r[rpc.metadata]`).
//
// On the wire metadata is a phon `Value` map; flags are well-known keys whose
// value is a list of the key names they apply to (`r[rpc.metadata.flags]`).

import type { Value } from "@bearcove/phon-schema";
import {
  type Metadata,
  MetadataKeys,
  metadataAddFlag,
  metadataIsSensitive,
  metadataIsNoPropagate,
} from "@bearcove/vox-wire";

/** A metadata value: string, u64 (bigint), or raw bytes. */
export type ClientMetadataValue = string | bigint | Uint8Array;

/**
 * Client-side metadata builder.
 *
 * `set()` for normal metadata; `setSensitive()` marks a key for redaction in logs
 * and traces (recorded under the `vox:sensitive` well-known key).
 */
export class ClientMetadata {
  private readonly map: Metadata = new Map();

  set(key: string, value: ClientMetadataValue): this {
    this.map.set(key, value as Value);
    return this;
  }

  /** r[impl rpc.metadata.flags.sensitive] */
  setSensitive(key: string, value: ClientMetadataValue): this {
    this.map.set(key, value as Value);
    metadataAddFlag(this.map, MetadataKeys.SENSITIVE, key);
    return this;
  }

  /** r[impl rpc.metadata.flags.no-propagate] */
  setNoPropagate(key: string, value: ClientMetadataValue): this {
    this.map.set(key, value as Value);
    metadataAddFlag(this.map, MetadataKeys.NO_PROPAGATE, key);
    return this;
  }

  get(key: string): Value | undefined {
    return this.map.get(key);
  }

  has(key: string): boolean {
    return this.map.has(key);
  }

  delete(key: string): boolean {
    return this.map.delete(key);
  }

  get size(): number {
    return this.map.size;
  }

  keys(): IterableIterator<string> {
    return this.map.keys();
  }

  entries(): IterableIterator<[string, Value]> {
    return this.map.entries();
  }

  isSensitive(key: string): boolean {
    return metadataIsSensitive(this.map, key);
  }

  isNoPropagate(key: string): boolean {
    return metadataIsNoPropagate(this.map, key);
  }

  /** The wire `Value` map (flags already folded into well-known keys). */
  toWire(): Metadata {
    return this.map;
  }

  clone(): ClientMetadata {
    const copy = new ClientMetadata();
    for (const [k, v] of this.map) copy.map.set(k, v);
    return copy;
  }

  static fromWire(metadata: Metadata): ClientMetadata {
    const m = new ClientMetadata();
    for (const [k, v] of metadata) m.map.set(k, v);
    return m;
  }
}

/** Convert a `ClientMetadata` to the wire `Value` map. */
export function clientMetadataToWire(metadata: ClientMetadata): Metadata {
  return metadata.toWire();
}
