import { describe, expect, it } from "vitest";
import { metadataKeyIsNoPropagate, metadataKeyIsRedacted } from "@bearcove/vox-wire";

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

  // r[verify rpc.metadata.sigils]
  it("treats metadata sigils as key-string conventions", () => {
    expect(metadataKeyIsRedacted("regular.metadata")).toBe(false);
    expect(metadataKeyIsNoPropagate("regular.metadata")).toBe(false);

    expect(metadataKeyIsRedacted("#sensitive.metadata")).toBe(true);
    expect(metadataKeyIsNoPropagate("#sensitive.metadata")).toBe(false);

    expect(metadataKeyIsRedacted("-no-propagate-metadata")).toBe(false);
    expect(metadataKeyIsNoPropagate("-no-propagate-metadata")).toBe(true);

    expect(metadataKeyIsRedacted("-#sensitive-and-no-propagate-metadata")).toBe(true);
    expect(metadataKeyIsNoPropagate("-#sensitive-and-no-propagate-metadata")).toBe(true);
  });
});
