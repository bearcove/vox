import { describe, expect, it } from "vitest";
import {
  type ConnectionSettings,
  emptyMetadata,
  encodeMessage,
  type Message,
  messageRequest,
  messageResponse,
} from "@bearcove/vox-wire";
import { BareConduit } from "./conduit.ts";
import { handshakeAsAcceptor, handshakeAsInitiator } from "./handshake.ts";
import {
  Session,
  ConnectionHandle,
  SessionError,
  session,
} from "./session.ts";
import type { MethodDescriptor } from "./channeling/index.ts";
import {
  resumeEchoRegistry,
  resumeEchoMethods,
  RESUME_ECHO_METHOD_ID,
} from "./resume_echo.fixture.ts";

const ECHO_METHOD_KEY = `0x${RESUME_ECHO_METHOD_ID.toString(16).padStart(16, "0")}`;
const ECHO_METHOD_SCHEMAS = resumeEchoMethods[ECHO_METHOD_KEY]!;

class MemoryLink {
  private readonly queue: Uint8Array[] = [];
  private waiting: ((value: Uint8Array | null) => void) | null = null;
  private closed = false;
  private readonly deliver: (payload: Uint8Array) => void;

  constructor(deliver: (payload: Uint8Array) => void) {
    this.deliver = deliver;
  }

  async send(payload: Uint8Array): Promise<void> {
    if (this.closed) {
      throw new Error("closed");
    }
    this.deliver(payload);
  }

  recv(): Promise<Uint8Array | null> {
    if (this.queue.length > 0) {
      return Promise.resolve(this.queue.shift()!);
    }
    if (this.closed) {
      return Promise.resolve(null);
    }
    return new Promise((resolve) => {
      this.waiting = resolve;
    });
  }

  push(payload: Uint8Array): void {
    if (this.closed) {
      return;
    }
    if (this.waiting) {
      const resolve = this.waiting;
      this.waiting = null;
      resolve(payload);
      return;
    }
    this.queue.push(payload);
  }

  close(): void {
    this.closed = true;
    const waiting = this.waiting;
    this.waiting = null;
    waiting?.(null);
  }

  isClosed(): boolean {
    return this.closed;
  }
}

function memoryLinkPair(): [MemoryLink, MemoryLink] {
  let left!: MemoryLink;
  let right!: MemoryLink;
  left = new MemoryLink((payload) => right.push(payload));
  right = new MemoryLink((payload) => left.push(payload));
  return [left, right];
}

async function withTimeout<T>(
  promise: Promise<T>,
  label: string,
  timeoutMs = 1_000,
): Promise<T> {
  const timeout = new Promise<never>((_, reject) => {
    setTimeout(() => reject(new Error(`timed out waiting for ${label}`)), timeoutMs);
  });
  return Promise.race([promise, timeout]);
}

async function establishPair(
  clientLink: MemoryLink,
  serverLink: MemoryLink,
): Promise<[Session, Session]> {
  const clientSettings: ConnectionSettings = {
    parity: { tag: "Odd" },
    max_concurrent_requests: 64,
    initial_channel_credit: 16,
  };
  const serverSettings: ConnectionSettings = {
    parity: { tag: "Even" },
    max_concurrent_requests: 64,
    initial_channel_credit: 16,
  };
  const [clientHandshake, serverHandshake] = await Promise.all([
    handshakeAsInitiator(clientLink, clientSettings),
    handshakeAsAcceptor(serverLink, serverSettings),
  ]);
  const clientConduit = new BareConduit(clientLink);
  const serverConduit = new BareConduit(serverLink);
  const clientSession = session.initiatorConduit(clientConduit, clientHandshake);
  const serverSession = session.acceptorConduit(serverConduit, serverHandshake);
  return [clientSession, serverSession];
}

const ECHO_METHOD: MethodDescriptor = {
  name: "echo",
  id: RESUME_ECHO_METHOD_ID,
};

describe("session", () => {
  // r[verify schema.exchange.required]
  it("tears down when a call arrives without an args schema binding", async () => {
    const [clientLink, serverLink] = memoryLinkPair();
    const [clientSession, serverSession] = await withTimeout(
      establishPair(clientLink, serverLink),
      "session establishment",
    );
    const serverRoot = serverSession.rootConnection();

    await clientLink.send(
      encodeMessage(
        messageRequest(1n, ECHO_METHOD.id, new Uint8Array(), emptyMetadata(), [], 0n, []),
      ),
    );

    await withTimeout(serverSession.closed(), "server protocol-error close");
    expect(serverRoot.isClosed()).toBe(true);

    clientLink.close();
    serverLink.close();
    clientSession.handle().shutdown();
    serverSession.handle().shutdown();
    await Promise.allSettled([clientSession.closed(), serverSession.closed()]);
  });

  // r[verify schema.exchange.required]
  it("tears down when a response arrives without a response schema binding", async () => {
    const [clientLink, serverLink] = memoryLinkPair();
    const [clientSession, serverSession] = await withTimeout(
      establishPair(clientLink, serverLink),
      "session establishment",
    );
    const clientRoot = clientSession.rootConnection();

    const call = clientRoot.caller().call({
      method: "Test.echo",
      args: { value: 55 },
      descriptor: ECHO_METHOD,
      methodSchemas: ECHO_METHOD_SCHEMAS,
      registry: resumeEchoRegistry,
    });
    await new Promise((resolve) => setTimeout(resolve, 0));

    await serverLink.send(
      encodeMessage(messageResponse(1n, new Uint8Array(), emptyMetadata(), 0n, [])),
    );

    await withTimeout(clientSession.closed(), "client protocol-error close");
    await expect(call).rejects.toBeInstanceOf(SessionError);
    expect(clientRoot.isClosed()).toBe(true);

    clientLink.close();
    serverLink.close();
    clientSession.handle().shutdown();
    serverSession.handle().shutdown();
    await Promise.allSettled([clientSession.closed(), serverSession.closed()]);
  });

  it("restarts channel flushing when new work arrives during a pending exit", async () => {
    const settings: ConnectionSettings = {
      parity: { tag: "Odd" },
      max_concurrent_requests: 64,
      initial_channel_credit: 16,
    };
    const sent: Message[] = [];
    const fakeSession = {
      sendMessage: async (message: Message) => {
        sent.push(message);
      },
    };
    const connection = new ConnectionHandle(
      fakeSession as never,
      0n,
      settings,
      settings,
    );

    let pollCount = 0;
    const fakeRegistry = {
      pollOutgoing() {
        pollCount += 1;
        if (pollCount === 1) {
          void connection.flushOutgoing();
          return { kind: "pending" } as const;
        }
        if (pollCount === 2) {
          return {
            kind: "data",
            channelId: 7n,
            payload: Uint8Array.of(1, 2, 3),
          } as const;
        }
        return { kind: "done" } as const;
      },
    };
    (
      connection as unknown as {
        channelRegistry: typeof fakeRegistry;
      }
    ).channelRegistry = fakeRegistry;

    await connection.flushOutgoing();

    expect(pollCount).toBe(3);
    expect(sent).toHaveLength(1);
    expect(sent[0]).toMatchObject({
      connection_id: 0n,
      payload: {
        tag: "ChannelMessage",
        value: {
          id: 7n,
          body: {
            tag: "Item",
          },
        },
      },
    });
    expect(
      Array.from(
        sent[0].payload.tag === "ChannelMessage"
          ? sent[0].payload.value.body.tag === "Item"
            ? sent[0].payload.value.body.value.item
            : new Uint8Array(0)
          : new Uint8Array(0),
      ),
    ).toEqual([1, 2, 3]);
  });
});
