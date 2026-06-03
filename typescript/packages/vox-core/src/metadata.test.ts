import { describe, expect, it } from "vitest";

import { ClientMetadata, clientMetadataToWire } from "./metadata.ts";

describe("ClientMetadata", () => {
  // r[verify schema.interaction.metadata]
  it("exposes metadata as a self-describing wire Value map", () => {
    const metadata = new ClientMetadata();
    const bytes = new Uint8Array([1, 2, 3]);

    metadata.set("trace-id", "abc");
    metadata.set("attempt", 7n);
    metadata.set("blob", bytes);

    const wire = clientMetadataToWire(metadata);

    expect(wire).toBe(metadata.toWire());
    expect(wire.get("trace-id")).toBe("abc");
    expect(wire.get("attempt")).toBe(7n);
    expect(wire.get("blob")).toBe(bytes);
  });
});
