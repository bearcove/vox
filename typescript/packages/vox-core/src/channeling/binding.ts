// Runtime channel binder — out-of-band index design, phon per-item codec.
//
// `Tx`/`Rx` arguments are opaque on the wire: each encodes only a `u32` index
// into `RequestCall.channels`, and the allocated `ChannelId`s travel out-of-band
// in that list (`r[rpc.channel.payload-encoding]`, `r[rpc.channel.allocation]`).
//
// For each channel argument we allocate a `ChannelId`, bind the *local-facing*
// handle (the pair of the one passed into the call) with a phon per-item codec
// keyed on the element root, and replace the argument with its wire-index bytes.

import { encodeTyped, decodeTyped } from "@bearcove/phon-engine";
import type { Registry } from "@bearcove/phon-schema";

import type { ChannelIdAllocator } from "./allocator.ts";
import type { ChannelRegistry } from "./registry.ts";
import type { Tx } from "./tx.ts";
import type { Rx } from "./rx.ts";
import type { PhonChannelMeta } from "../schema_tracker.ts";

/** The 4-byte little-endian phon-compact encoding of a `u32` wire index. */
function wireIndexBytes(index: number): Uint8Array {
  const out = new Uint8Array(4);
  new DataView(out.buffer).setUint32(0, index, true);
  return out;
}

/** A phon serializer for a channel element type identified by `elementRoot`. */
function makeSerialize(elementRoot: bigint, registry: Registry): (value: unknown) => Uint8Array {
  return (value) => encodeTyped(value as never, elementRoot, registry);
}

/** A phon deserializer for a channel element type (writer == reader == element). */
function makeDeserialize(elementRoot: bigint, registry: Registry): (bytes: Uint8Array) => unknown {
  return (bytes) => decodeTyped(bytes, elementRoot, elementRoot, registry) as unknown;
}

/** Per-direction initial credit windows for a bound channel. */
export interface ChannelCredit {
  /** Credit we may spend sending (the peer's advertised initial grant). */
  outgoing: number;
  /** Credit window we offer the peer when receiving (drives re-grant cadence). */
  incoming: number;
}

export interface BoundChannels {
  /** `RequestCall.channels`, in wire-index (allocation) order. */
  channels: bigint[];
  /** The args values with each `Tx`/`Rx` replaced by its wire-index `Bytes`. */
  values: unknown[];
  /** Finalize bound handles (completes any retry binding). */
  finalize: () => void;
}

/**
 * Bind the `Tx`/`Rx` channels in a call's argument list. `channelMetas` comes
 * from the generated `{service}Methods[...].channels` table; each entry's
 * `index` is the argument position and `direction` is the method-signature point
 * of view (so a `tx` arg means the *callee* sends and the caller receives).
 */
export function bindPhonChannels(
  args: unknown[],
  channelMetas: PhonChannelMeta[],
  allocator: ChannelIdAllocator,
  channelRegistry: ChannelRegistry,
  registry: Registry,
  credit: ChannelCredit,
): BoundChannels {
  if (channelMetas.length === 0) {
    return { channels: [], values: args, finalize: () => {} };
  }

  const values = [...args];
  const channels: bigint[] = [];
  const bound: Array<Tx<unknown> | Rx<unknown>> = [];

  // Allocate in argument-position order so the wire index is stable.
  const metas = [...channelMetas].sort((a, b) => a.index - b.index);
  for (const meta of metas) {
    const handle = values[meta.index] as Tx<unknown> | Rx<unknown>;
    const channelId = allocator.next();
    const wireIndex = channels.length;
    channels.push(channelId);
    bindOne(handle, meta, channelId, channelRegistry, registry, credit);
    values[meta.index] = wireIndexBytes(wireIndex);
    bound.push(handle);
  }

  const finalize = (): void => {
    for (const handle of bound) {
      const pair = (handle as { _pair?: { finishRetryBinding?: () => void } })._pair;
      pair?.finishRetryBinding?.();
      (handle as { finishRetryBinding?: () => void }).finishRetryBinding?.();
    }
  };

  return { channels, values, finalize };
}

function bindOne(
  handle: Tx<unknown> | Rx<unknown>,
  meta: PhonChannelMeta,
  channelId: bigint,
  channelRegistry: ChannelRegistry,
  registry: Registry,
  credit: ChannelCredit,
): void {
  if (meta.direction === "tx") {
    // Method wants a `Tx` (callee sends). The caller passed a `Tx` and keeps the
    // paired `Rx` — the caller receives. Bind that pair for INCOMING.
    const tx = handle as Tx<unknown>;
    const rx = (tx as { _pair?: Rx<unknown> })._pair;
    const deserialize = makeDeserialize(meta.elementRoot, registry);
    if (rx) {
      if (rx.isBound) rx.rebind(channelId, channelRegistry, deserialize, credit.incoming);
      else rx.bind(channelId, channelRegistry, deserialize, credit.incoming);
    }
    return;
  }

  // Method wants an `Rx` (callee receives). The caller passed an `Rx` and keeps
  // the paired `Tx` — the caller sends. Bind that pair for OUTGOING.
  const rx = handle as Rx<unknown>;
  const tx = (rx as { _pair?: Tx<unknown> })._pair;
  const serialize = makeSerialize(meta.elementRoot, registry);
  if (tx) {
    if (tx.isBound) tx.rebind(channelId, channelRegistry, serialize, credit.outgoing);
    else tx.bind(channelId, channelRegistry, serialize, credit.outgoing);
  }
}
