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
import { Driver, type Dispatcher } from "./driver.ts";
import { handshakeAsAcceptor, handshakeAsInitiator } from "./handshake.ts";
import { RequestContext } from "./request_context.ts";
import {
  Session,
  ConnectionHandle,
  SessionError,
  SessionRegistry,
  session,
  type SessionAcceptOutcome,
  type SessionHandle,
} from "./session.ts";
import type { MethodDescriptor, ServiceDescriptor } from "./channeling/index.ts";
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

function makeDeferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
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

async function resumeWhenReady(
  handle: SessionHandle,
  link: MemoryLink,
  isInitiator: boolean,
): Promise<void> {
  const settings: ConnectionSettings = {
    parity: isInitiator ? { tag: "Odd" } : { tag: "Even" },
    max_concurrent_requests: 64,
    initial_channel_credit: 16,
  };
  const resumeKey = handle.sessionResumeKey();
  const handshake = isInitiator
    ? await handshakeAsInitiator(link, settings, resumeKey)
    : await handshakeAsAcceptor(link, settings);
  void handshake;
  const conduit = new BareConduit(link);
  for (let attempt = 0; attempt < 50; attempt++) {
    try {
      await handle.resume(conduit);
      return;
    } catch (error) {
      if (
        !(error instanceof SessionError)
        || !error.message.includes("resume is only valid while the session is disconnected")
      ) {
        throw error;
      }
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }
  throw new Error("session never became disconnected");
}

async function establishPair(
  clientLink: MemoryLink,
  serverLink: MemoryLink,
  opts: { resumable?: boolean } = {},
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
    handshakeAsAcceptor(serverLink, serverSettings, opts.resumable ?? false),
  ]);
  const clientConduit = new BareConduit(clientLink);
  const serverConduit = new BareConduit(serverLink);
  const clientSession = session.initiatorConduit(clientConduit, clientHandshake, { resumable: opts.resumable ?? false });
  const serverSession = session.acceptorConduit(serverConduit, serverHandshake, { resumable: opts.resumable ?? false });
  return [clientSession, serverSession];
}

const ECHO_METHOD: MethodDescriptor = {
  name: "echo",
  id: RESUME_ECHO_METHOD_ID,
};

function descriptorFor(method: MethodDescriptor): ServiceDescriptor {
  return {
    service_name: "ResumeEcho",
    send_schemas: resumeEchoMethods,
    registry: resumeEchoRegistry,
    methods: new Map([[method.id, method]]),
  };
}

describe("session resumption", () => {
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

  it("fails an in-flight attempt across manual resume but accepts fresh calls", async () => {
    const [clientLink1, serverLink1] = memoryLinkPair();
    const started = makeDeferred<void>();
    const release = makeDeferred<void>();
    let runs = 0;

    const dispatcher: Dispatcher = {
      getDescriptor() {
        return descriptorFor(ECHO_METHOD);
      },
      async dispatch(_context: RequestContext, _method, args, call) {
        runs += 1;
        if (runs === 1) {
          started.resolve();
          await release.promise;
        }
        call.reply(args[0]);
      },
    };

    const [clientSession, serverSession] = await withTimeout(
      establishPair(clientLink1, serverLink1, { resumable: true }),
      "initial session establishment",
    );
    const serverDriver = new Driver(serverSession.rootConnection(), dispatcher);
    const serverRun = serverDriver.run();

    const call = clientSession.rootConnection().caller().call({
      method: "Test.echo",
      args: { value: 55 },
      descriptor: ECHO_METHOD,
      methodSchemas: ECHO_METHOD_SCHEMAS,
      registry: resumeEchoRegistry,
    });

    await withTimeout(started.promise, "handler start");

    clientLink1.close();
    serverLink1.close();

    const [clientLink2, serverLink2] = memoryLinkPair();

    await withTimeout(
      Promise.all([
        resumeWhenReady(serverSession.handle(), serverLink2, false),
        resumeWhenReady(clientSession.handle(), clientLink2, true),
      ]),
      "session resume",
    );

    await expect(withTimeout(call, "failed in-flight attempt")).rejects.toBeInstanceOf(
      SessionError,
    );

    release.resolve();

    await expect(
      withTimeout(
        clientSession.rootConnection().caller().call({
          method: "Test.echo",
          args: { value: 56 },
          descriptor: ECHO_METHOD,
          methodSchemas: ECHO_METHOD_SCHEMAS,
          registry: resumeEchoRegistry,
        }),
        "fresh post-resume call",
      ),
    ).resolves.toBe(56);
    expect(runs).toBe(2);

    clientLink2.close();
    serverLink2.close();
    clientSession.handle().shutdown();
    serverSession.handle().shutdown();

    await Promise.allSettled([serverRun, serverSession.closed(), clientSession.closed()]);
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

  it("fails an in-flight attempt across registry-driven acceptor resume", async () => {
    const registry = new SessionRegistry();
    const [clientLink1, serverLink1] = memoryLinkPair();
    const started = makeDeferred<void>();
    const release = makeDeferred<void>();

    const dispatcher: Dispatcher = {
      getDescriptor() {
        return descriptorFor(ECHO_METHOD);
      },
      async dispatch(_context: RequestContext, _method, args, call) {
        started.resolve();
        await release.promise;
        call.reply(args[0]);
      },
    };

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
    handshakeAsInitiator(clientLink1, clientSettings),
    handshakeAsAcceptor(serverLink1, serverSettings, true),
  ]);
    const clientConduit1 = new BareConduit(clientLink1);
    const serverConduit1 = new BareConduit(serverLink1);
    const clientSession = session.initiatorConduit(clientConduit1, clientHandshake, { resumable: true });
    const firstAccepted = session.acceptorOrResume(serverConduit1, serverHandshake, registry, { resumable: true });
    expect((firstAccepted as SessionAcceptOutcome).tag).toBe("Established");
    const firstSession = (firstAccepted as Extract<SessionAcceptOutcome, { tag: "Established" }>).session;
    const serverDriver = new Driver(firstSession.rootConnection(), dispatcher);
    const serverRun = serverDriver.run();

    const call = clientSession.rootConnection().caller().call({
      method: "Test.echo",
      args: { value: 66 },
      descriptor: ECHO_METHOD,
      methodSchemas: ECHO_METHOD_SCHEMAS,
      registry: resumeEchoRegistry,
    });

    await withTimeout(started.promise, "handler start");

    clientLink1.close();
    serverLink1.close();

    const [clientLink2, serverLink2] = memoryLinkPair();

    const [serverHandshake2, clientLink2Settled] = await Promise.all([
      handshakeAsAcceptor(serverLink2, serverSettings, true, clientSession.handle().sessionResumeKey()),
      resumeWhenReady(clientSession.handle(), clientLink2, true).then(() => null),
    ]);
    void clientLink2Settled;
    const serverConduit2 = new BareConduit(serverLink2);
    const acceptResult = session.acceptorOrResume(serverConduit2, serverHandshake2, registry, { resumable: true });

    expect(acceptResult.tag).toBe("Resumed");

    release.resolve();

    await expect(withTimeout(call, "registry-resumed in-flight attempt")).rejects.toBeInstanceOf(
      SessionError,
    );

    clientLink2.close();
    serverLink2.close();
    clientSession.handle().shutdown();
    firstSession.handle().shutdown();

    await Promise.allSettled([serverRun, firstSession.closed(), clientSession.closed()]);
  });
});
