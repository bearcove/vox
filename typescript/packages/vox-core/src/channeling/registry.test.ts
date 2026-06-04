import { describe, expect, it } from "vitest";

import { ChannelRegistry } from "./registry.ts";

describe("ChannelRegistry", () => {
  // r[verify rpc.channel.binding]
  // r[verify rpc.channel.binding.callee-args]
  // r[verify rpc.channel.binding.callee-args.rx]
  // r[verify rpc.channel.delivery.reliable]
  // r[verify rpc.channel.item]
  // r[verify rpc.flow-control.credit.initial]
  it("buffers incoming data until the receiver is registered", async () => {
    const registry = new ChannelRegistry();
    const channelId = 7n;
    const first = Uint8Array.of(1, 2, 3);
    const second = Uint8Array.of(4, 5, 6);

    registry.routeData(channelId, first);
    registry.routeData(channelId, second);

    const rx = registry.registerIncoming(channelId, 2);
    await expect(rx.recv()).resolves.toEqual(first);
    await expect(rx.recv()).resolves.toEqual(second);
  });

  // r[verify rpc.channel.binding.callee-args.tx]
  // r[verify rpc.flow-control.credit.initial]
  it("registers outgoing Tx handles by channel id", async () => {
    const registry = new ChannelRegistry();
    const channelId = 11n;
    const payload = Uint8Array.of(8, 9);

    const tx = registry.registerOutgoing(channelId, 1);
    await tx.sendData(payload);

    expect(registry.pollOutgoing()).toEqual({ kind: "data", channelId, payload });
  });

  // r[verify rpc.flow-control.credit]
  // r[verify rpc.flow-control.credit.exhaustion]
  it("blocks outgoing data when credit is exhausted until credit is granted", async () => {
    const registry = new ChannelRegistry();
    const channelId = 13n;
    const sender = registry.registerOutgoing(channelId, 1);
    const first = Uint8Array.of(1);
    const second = Uint8Array.of(2);

    await sender.sendData(first);
    let sentSecond = false;
    const blocked = sender.sendData(second).then(() => {
      sentSecond = true;
    });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(sentSecond).toBe(false);
    expect(registry.pollOutgoing()).toEqual({ kind: "data", channelId, payload: first });

    registry.grantCredit(channelId, 1);
    await blocked;

    expect(sentSecond).toBe(true);
    expect(registry.pollOutgoing()).toEqual({ kind: "data", channelId, payload: second });
  });

  // r[verify rpc.flow-control.credit.grant]
  // r[verify rpc.flow-control.credit.grant.additive]
  it("queues additive credit grants as incoming items are consumed", async () => {
    const registry = new ChannelRegistry();
    const channelId = 15n;
    const payload = Uint8Array.of(3);
    const rx = registry.registerIncoming(channelId, 2);

    registry.routeData(channelId, payload);

    await expect(rx.recv()).resolves.toEqual(payload);
    expect(registry.pollOutgoing()).toEqual({ kind: "credit", channelId, additional: 1 });
  });

  // r[verify rpc.channel.close]
  // r[verify rpc.channel.lifecycle]
  // r[verify rpc.channel.reset]
  it("preserves buffered terminal close before the receiver is registered", async () => {
    const registry = new ChannelRegistry();
    const channelId = 9n;
    const payload = Uint8Array.of(42);

    registry.routeData(channelId, payload);
    registry.close(channelId);

    const rx = registry.registerIncoming(channelId, 1);
    await expect(rx.recv()).resolves.toEqual(payload);
    await expect(rx.recv()).resolves.toBeNull();
    expect(() => registry.routeData(channelId, Uint8Array.of(7))).toThrow(/data after close/i);
  });
});
