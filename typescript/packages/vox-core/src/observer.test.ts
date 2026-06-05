import { describe, expect, it } from "vitest";
import { observerMetricLabels } from "./observer.ts";

describe("observerMetricLabels", () => {
  // r[verify rpc.observability.low-cardinality]
  it("keeps only the default low-cardinality metric labels", () => {
    const labels = observerMetricLabels({
      service: "Echo",
      method: "echo",
      side: "client",
      outcome: "ok",
      error_kind: "",
      channel_direction: "tx",
      connection_id: "13",
      request_id: "21",
      channel_id: "34",
      peer_address: "/tmp/vox.sock",
      metadata: "tenant",
    } as Parameters<typeof observerMetricLabels>[0]);

    expect(labels).toEqual({
      service: "Echo",
      method: "echo",
      side: "client",
      outcome: "ok",
      channel_direction: "tx",
    });
    expect(Object.keys(labels)).not.toContain("connection_id");
    expect(Object.keys(labels)).not.toContain("request_id");
    expect(Object.keys(labels)).not.toContain("channel_id");
    expect(Object.keys(labels)).not.toContain("metadata");
  });
});
