import { describe, expect, it } from "vitest";
import { SchemaSendTracker } from "./schema_tracker.ts";

describe("SchemaSendTracker", () => {
  // r[verify schema.format.delivery]
  it("advertises a method-direction binding once", () => {
    const tracker = new SchemaSendTracker();
    const closure = "010203";

    expect(tracker.prepareSchemas(7n, "args", closure)).toEqual([1, 2, 3]);
    expect(tracker.prepareSchemas(7n, "args", closure)).toEqual([]);
    expect(tracker.prepareSchemas(7n, "response", closure)).toEqual([1, 2, 3]);
  });
});
