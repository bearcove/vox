import { describe, expect, it } from "vitest";
import type { Registry } from "@bearcove/phon-schema";
import type { SchemaTracker } from "../schema_tracker.ts";
import { ChannelIdAllocator } from "./allocator.ts";
import { bindPhonChannels } from "./binding.ts";
import { channel } from "./pair.ts";
import { ChannelRegistry } from "./registry.ts";
import { Role } from "./types.ts";

describe("bindPhonChannels", () => {
  // r[verify schema.exchange.channels.tx-args]
  it("uses lazily advertised auxiliary schemas for caller-side channel receives", async () => {
    const [tx, rx] = channel<unknown>();
    const registry = new ChannelRegistry();
    const allocator = new ChannelIdAllocator(Role.Initiator);
    const seen: Array<[bigint, string, string, bigint]> = [];
    const tracker = {
      buildAuxiliaryDecoder(
        methodId: bigint,
        direction: "args" | "response",
        role: string,
        readerRoot: bigint,
      ) {
        seen.push([methodId, direction, role, readerRoot]);
        return (bytes: Uint8Array) => `aux:${bytes[0]}`;
      },
    } as unknown as SchemaTracker;

    const bound = bindPhonChannels(
      [tx],
      [{ index: 0, direction: "tx", elementRoot: 123n }],
      allocator,
      registry,
      {} as Registry,
      { incoming: 4, outgoing: 4 },
      { methodId: 55n, direction: "args", tracker },
    );

    registry.routeData(bound.channels[0]!, Uint8Array.of(9));

    await expect(rx.recv()).resolves.toBe("aux:9");
    expect(seen).toEqual([[55n, "args", "channel.arg.0.tx.element", 123n]]);
  });
});
