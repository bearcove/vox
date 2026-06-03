import { describe, expect, it } from "vitest";
import { emptyMetadata } from "@bearcove/vox-wire";
import { hexToBytes, type Registry } from "@bearcove/phon-schema";
import { Driver } from "./driver.ts";
import {
  SchemaSendTracker,
  type PhonChannelMeta,
  type PhonMethodSchemas,
  type SchemaTracker,
} from "./schema_tracker.ts";
import type { MethodDescriptor, TaskMessage } from "./channeling/index.ts";
import {
  sessionEchoRegistry,
  sessionEchoMethods,
  SESSION_ECHO_METHOD_ID,
} from "./session_echo.fixture.ts";

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

const ECHO_METHOD_KEY = `0x${SESSION_ECHO_METHOD_ID.toString(16).padStart(16, "0")}`;
const ECHO_METHOD_SCHEMAS = sessionEchoMethods[ECHO_METHOD_KEY]!;
const ECHO_METHOD: MethodDescriptor = {
  name: "echo",
  id: SESSION_ECHO_METHOD_ID,
};

describe("Driver channel schema exchange", () => {
  // r[verify schema.exchange.callee]
  it("advertises response schemas with the first callee response", async () => {
    const sent: Array<{ requestId: bigint; schemas: number[] }> = [];
    const schemaSendTracker = new SchemaSendTracker();
    const driver = new Driver(
      {
        currentEpoch: () => 0,
        getSchemaSendTracker: () => schemaSendTracker,
        getSchemaTracker: () => ({
          requireReceived() {},
        }),
        sendResponse: async (
          requestId: bigint,
          _payload: Uint8Array,
          _metadata: unknown,
          _channels: bigint[],
          schemas: number[],
        ) => {
          sent.push({ requestId, schemas });
        },
      } as never,
      {
        getDescriptor: () => ({
          service_name: "Test",
          send_schemas: { [ECHO_METHOD_KEY]: ECHO_METHOD_SCHEMAS },
          registry: sessionEchoRegistry,
          methods: new Map([[ECHO_METHOD.id, ECHO_METHOD]]),
        }),
        dispatch: async (_context, _method, _args, call) => {
          call.reply(123);
        },
      },
    ) as unknown as {
      handleCall(call: {
        requestId: bigint;
        methodId: bigint;
        args: Uint8Array;
        channels: bigint[];
        metadata: ReturnType<typeof emptyMetadata>;
        connectionEpoch: number;
      }): Promise<void>;
    };

    await driver.handleCall({
      requestId: 9n,
      methodId: ECHO_METHOD.id,
      args: new Uint8Array(),
      channels: [],
      metadata: emptyMetadata(),
      connectionEpoch: 0,
    });

    expect(sent).toHaveLength(1);
    expect(sent[0]).toEqual({
      requestId: 9n,
      schemas: Array.from(hexToBytes(ECHO_METHOD_SCHEMAS.responseSchemaClosure)),
    });
  });

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
