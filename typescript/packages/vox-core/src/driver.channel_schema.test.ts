import { describe, expect, it } from "vitest";
import type { Registry } from "@bearcove/phon-schema";
import { Driver } from "./driver.ts";
import {
  SchemaSendTracker,
  type PhonChannelMeta,
  type PhonMethodSchemas,
  type SchemaTracker,
} from "./schema_tracker.ts";
import type { MethodDescriptor, TaskMessage } from "./channeling/index.ts";

const METHOD: MethodDescriptor = {
  name: "stream",
  id: 77n,
};

const METHOD_SCHEMAS: PhonMethodSchemas = {
  argsRoot: 1n,
  argsSchemaClosure: "010203",
  okRoot: 2n,
  responseRoot: 3n,
  responseSchemaClosure: "040506",
  channels: [{ index: 0, direction: "tx", elementRoot: 9n }],
};

describe("Driver channel schema exchange", () => {
  // r[verify schema.exchange.channels.tx-args]
  it("advertises args schemas before the first server-written channel item", () => {
    const sent: TaskMessage[] = [];
    const driver = new Driver(
      {
        getSchemaSendTracker: () => new SchemaSendTracker(),
      } as never,
      {
        getDescriptor: () => ({
          service_name: "Test",
          send_schemas: {},
          registry: {} as never,
          methods: new Map(),
        }),
        dispatch: async () => {},
      },
    ) as unknown as {
      argsSchemaAdvertisingTaskSender(
        method: MethodDescriptor,
        methodSchemas: PhonMethodSchemas,
        taskSender: (message: TaskMessage) => void,
      ): (message: TaskMessage) => void;
    };
    const sender = driver.argsSchemaAdvertisingTaskSender(METHOD, METHOD_SCHEMAS, (message) => {
      sent.push(message);
    });

    sender({ kind: "data", channelId: 11n, payload: Uint8Array.of(1) });
    sender({ kind: "data", channelId: 11n, payload: Uint8Array.of(2) });

    expect(sent.map((message) => message.kind)).toEqual(["schema", "data", "data"]);
    expect(sent[0]).toMatchObject({
      kind: "schema",
      methodId: 77n,
      direction: "args",
      schemas: Uint8Array.of(1, 2, 3),
    });
  });

  // r[verify schema.exchange.channels.rx-args]
  it("decodes server Rx channel items through the caller auxiliary root", () => {
    const seen: Array<[bigint, string, string, bigint]> = [];
    const tracker = {
      buildAuxiliaryDecoder(
        methodId: bigint,
        direction: "args" | "response",
        role: string,
        readerRoot: bigint,
      ) {
        seen.push([methodId, direction, role, readerRoot]);
        return (bytes: Uint8Array) => `rx:${bytes[0]}`;
      },
    } as unknown as SchemaTracker;
    const driver = new Driver(
      {
        getSchemaTracker: () => tracker,
      } as never,
      {
        getDescriptor: () => ({
          service_name: "Test",
          send_schemas: {},
          registry: {} as never,
          methods: new Map(),
        }),
        dispatch: async () => {},
      },
    ) as unknown as {
      channelElementDeserializer(
        method: MethodDescriptor,
        channel: PhonChannelMeta,
        registry: Registry,
      ): (bytes: Uint8Array) => unknown;
    };

    const decoder = driver.channelElementDeserializer(
      METHOD,
      { index: 1, direction: "rx", elementRoot: 456n },
      {} as Registry,
    );

    expect(decoder(Uint8Array.of(8))).toBe("rx:8");
    expect(seen).toEqual([[77n, "args", "channel.arg.1.rx.element", 456n]]);
  });
});
