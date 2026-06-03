import {
  emptyMetadata,
} from "@bearcove/vox-wire";
import { encodeTyped, decodeTyped } from "@bearcove/phon-engine";
import type { Registry } from "@bearcove/phon-schema";
import {
  type MethodDescriptor,
  type VoxCall,
  type ServiceDescriptor,
  type TaskMessage,
  type TaskSender,
  createServerTx,
  createServerRx,
  DEFAULT_INITIAL_CREDIT,
} from "./channeling/index.ts";
import type { ServiceSendSchemas } from "./channeling/descriptor.ts";
import type { PhonChannelMeta, PhonMethodSchemas } from "./schema_tracker.ts";
import { Extensions } from "./middleware.ts";
import { RequestContext } from "./request_context.ts";
import { metadataOperationId } from "./retry.ts";
import { type ServerCallOutcome, type ServerMiddleware } from "./server_middleware.ts";
import type { ConnectionHandle, IncomingCall } from "./session.ts";
import { voxLogger } from "./logger.ts";

export interface Dispatcher {
  getDescriptor(): ServiceDescriptor;
  dispatch(
    context: RequestContext,
    method: MethodDescriptor,
    args: unknown[],
    call: VoxCall,
  ): Promise<void>;
}

interface OperationSignature {
  methodId: bigint;
  args: Uint8Array;
}

export interface SealedOperationResponse {
  payload: Uint8Array;
  responseSchemaClosure: string;
}

/**
 * Interface for operation state backing — mirrors Rust's `OperationStore` trait.
 *
 * The default implementation is `InMemoryOperationStore`.
 * Applications that want stronger retention or durability can provide their own.
 */
export interface OperationStore {
  admit(
    operationId: bigint,
    methodId: bigint,
    args: Uint8Array,
    retry: MethodDescriptor["retry"],
    requestId: bigint,
  ): OperationAdmit;

  seal(
    operationId: bigint,
    ownerRequestId: bigint,
    response: SealedOperationResponse,
  ): bigint[];

  failWithoutReply(operationId: bigint, ownerRequestId: bigint): bigint[];

  cancel(requestId: bigint): OperationCancel;
}

interface StoredOperation {
  signature: OperationSignature;
  retry: MethodDescriptor["retry"];
}

interface LiveOperation {
  kind: "live";
  stored: StoredOperation;
  ownerRequestId: bigint;
  waiters: bigint[];
}

interface ReleasedOperation {
  kind: "released";
  stored: StoredOperation;
}

interface IndeterminateOperation {
  kind: "indeterminate";
  stored: StoredOperation;
}

interface SealedOperation {
  kind: "sealed";
  stored: StoredOperation;
  response: SealedOperationResponse;
}

type OperationState =
  | LiveOperation
  | ReleasedOperation
  | IndeterminateOperation
  | SealedOperation;

type OperationAdmit =
  | { kind: "start" }
  | { kind: "attached" }
  | { kind: "replay"; response: SealedOperationResponse }
  | { kind: "conflict" }
  | { kind: "indeterminate" };

type OperationCancel =
  | { kind: "none" }
  | { kind: "detach" }
  | { kind: "release"; ownerRequestId: bigint; waiters: bigint[] };

/** The `send_schemas` map key for a method id (matches codegen `0x{:016x}`). */
function methodKey(id: bigint): string {
  return `0x${id.toString(16).padStart(16, "0")}`;
}

/** Read a little-endian `u32` from a 4-byte phon-compact scalar. */
function readU32LE(bytes: Uint8Array): number {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0, true);
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) {
    return false;
  }
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) {
      return false;
    }
  }
  return true;
}

function channelElementRole(meta: PhonChannelMeta): string {
  return `channel.arg.${meta.index}.${meta.direction}.element`;
}

function sameSignature(
  signature: OperationSignature,
  methodId: bigint,
  args: Uint8Array,
): boolean {
  return signature.methodId === methodId && sameBytes(signature.args, args);
}

export class InMemoryOperationStore implements OperationStore {
  private readonly states = new Map<bigint, OperationState>();
  private readonly requestToOperation = new Map<bigint, bigint>();

  admit(
    operationId: bigint,
    methodId: bigint,
    args: Uint8Array,
    retry: MethodDescriptor["retry"],
    requestId: bigint,
  ): OperationAdmit {
    const signature: OperationSignature = {
      methodId,
      args: args.slice(),
    };
    const existing = this.states.get(operationId);
    if (!existing) {
      this.requestToOperation.set(requestId, operationId);
      this.states.set(operationId, {
        kind: "live",
        stored: { signature, retry },
        ownerRequestId: requestId,
        waiters: [requestId],
      });
      return { kind: "start" };
    }

    switch (existing.kind) {
      case "live":
        if (!sameSignature(existing.stored.signature, methodId, args)) {
          return { kind: "conflict" };
        }
        existing.waiters.push(requestId);
        this.requestToOperation.set(requestId, operationId);
        return { kind: "attached" };
      case "sealed":
        if (!sameSignature(existing.stored.signature, methodId, args)) {
          return { kind: "conflict" };
        }
        return {
          kind: "replay",
          response: {
            payload: existing.response.payload.slice(),
            responseSchemaClosure: existing.response.responseSchemaClosure,
          },
        };
      case "released":
      case "indeterminate":
        if (!sameSignature(existing.stored.signature, methodId, args) || !existing.stored.retry.idem) {
          return sameSignature(existing.stored.signature, methodId, args)
            ? { kind: "indeterminate" }
            : { kind: "conflict" };
        }
        this.requestToOperation.set(requestId, operationId);
        this.states.set(operationId, {
          kind: "live",
          stored: { signature, retry: existing.stored.retry },
          ownerRequestId: requestId,
          waiters: [requestId],
        });
        return { kind: "start" };
    }
  }

  seal(
    operationId: bigint,
    ownerRequestId: bigint,
    response: SealedOperationResponse,
  ): bigint[] {
    const existing = this.states.get(operationId);
    if (!existing || existing.kind !== "live" || existing.ownerRequestId !== ownerRequestId) {
      return [];
    }
    for (const waiter of existing.waiters) {
      this.requestToOperation.delete(waiter);
    }
    this.states.set(operationId, {
      kind: "sealed",
      stored: existing.stored,
      response: {
        payload: response.payload.slice(),
        responseSchemaClosure: response.responseSchemaClosure,
      },
    });
    return [...existing.waiters];
  }

  failWithoutReply(operationId: bigint, ownerRequestId: bigint): bigint[] {
    const existing = this.states.get(operationId);
    if (!existing || existing.kind !== "live" || existing.ownerRequestId !== ownerRequestId) {
      return [];
    }
    for (const waiter of existing.waiters) {
      this.requestToOperation.delete(waiter);
    }
    this.states.set(operationId, existing.stored.retry.persist
      ? { kind: "indeterminate", stored: existing.stored }
      : { kind: "released", stored: existing.stored });
    return [...existing.waiters];
  }

  cancel(requestId: bigint): OperationCancel {
    const operationId = this.requestToOperation.get(requestId);
    if (operationId === undefined) {
      return { kind: "none" };
    }
    const existing = this.states.get(operationId);
    if (!existing || existing.kind !== "live") {
      this.requestToOperation.delete(requestId);
      return { kind: "none" };
    }
    if (existing.stored.retry.persist) {
      if (existing.ownerRequestId === requestId) {
        return { kind: "none" };
      }
      existing.waiters = existing.waiters.filter((candidate) => candidate !== requestId);
      this.requestToOperation.delete(requestId);
      return { kind: "detach" };
    }
    for (const waiter of existing.waiters) {
      this.requestToOperation.delete(waiter);
    }
    this.states.set(operationId, { kind: "released", stored: existing.stored });
    return {
      kind: "release",
      ownerRequestId: existing.ownerRequestId,
      waiters: [...existing.waiters],
    };
  }
}

class VoxCallImpl implements VoxCall {
  private replied = false;

  private readonly method: MethodDescriptor;
  private readonly requestId: bigint;
  private readonly taskSender: TaskSender;
  private readonly operations: OperationStore;
  private readonly operationId: bigint | undefined;
  private readonly schemaSendTracker: import("./schema_tracker.ts").SchemaSendTracker;
  private readonly methodSchemas: PhonMethodSchemas;
  private readonly registry: Registry;

  constructor(
    method: MethodDescriptor,
    requestId: bigint,
    taskSender: TaskSender,
    operations: OperationStore,
    operationId: bigint | undefined,
    schemaSendTracker: import("./schema_tracker.ts").SchemaSendTracker,
    methodSchemas: PhonMethodSchemas,
    registry: Registry,
  ) {
    this.method = method;
    this.requestId = requestId;
    this.taskSender = taskSender;
    this.operations = operations;
    this.operationId = operationId;
    this.schemaSendTracker = schemaSendTracker;
    this.methodSchemas = methodSchemas;
    this.registry = registry;
  }

  didReply(): boolean {
    return this.replied;
  }

  reply(value: unknown): void {
    if (this.replied) {
      return;
    }
    this.replied = true;
    // A void handler returns `undefined`; phon's unit Value is `null`. Coerce so a
    // `Result<(), E>` Ok payload encodes (`??` keeps falsy values like `0`/`false`).
    const payload = this.encodeResponse({ tag: "Ok", value: value ?? null });
    this.sendPayload(payload);
  }

  replyErr(error: unknown): void {
    if (this.replied) {
      return;
    }
    this.replied = true;
    const payload = this.encodeResponse({ tag: "Err", value: { tag: "User", value: error } });
    this.sendPayload(payload);
  }

  replyInternalError(message = "Invalid payload"): void {
    if (this.replied) {
      return;
    }
    this.replied = true;
    const payload = this.encodeResponse({
      tag: "Err",
      value: { tag: "InvalidPayload", value: message },
    });
    this.sendPayload(payload);
  }

  /**
   * Encode a `Result<T, VoxError<E>>` response payload as phon bytes against the
   * method's `responseRoot` (`r[zerocopy.payload]`). The `{ tag, value }` shape
   * mirrors the Rust `RequestResponse.ret`.
   */
  private encodeResponse(result: {
    tag: "Ok" | "Err";
    value: unknown;
  }): Uint8Array {
    return encodeTyped(result as never, this.methodSchemas.responseRoot, this.registry);
  }

  /**
   * The phon schema-closure bytes to advertise for this method's response
   * binding, or undefined when already sent on this connection
   * (`r[schema.exchange.idempotent]`).
   */
  private prepareResponseSchemas(): Uint8Array | undefined {
    const nums = this.schemaSendTracker.prepareSchemas(
      this.method.id,
      "response",
      this.methodSchemas.responseSchemaClosure,
    );
    return nums.length > 0 ? new Uint8Array(nums) : undefined;
  }

  private sendPayload(payload: Uint8Array): void {
    if (this.operationId === undefined) {
      this.taskSender({
        kind: "response",
        requestId: this.requestId,
        payload,
        schemas: this.prepareResponseSchemas(),
      });
      return;
    }
    // r[impl schema.interaction.retry]
    const response: SealedOperationResponse = {
      payload,
      responseSchemaClosure: this.methodSchemas.responseSchemaClosure,
    };
    const waiters = this.operations.seal(this.operationId, this.requestId, response);
    for (const waiter of waiters) {
      this.taskSender({
        kind: "response",
        requestId: waiter,
        payload: payload.slice(),
        schemas: this.prepareResponseSchemas(),
      });
    }
  }
}

export class Driver {
  private readonly connection: ConnectionHandle;
  private readonly dispatcher: Dispatcher;
  private readonly middlewares: ServerMiddleware[];
  private readonly taskQueue: TaskMessage[] = [];
  private readonly operations: OperationStore;
  private inFlight = new Set<Promise<void>>();
  private wakeupResolve: (() => void) | null = null;

  static new(
    connection: ConnectionHandle,
    dispatcher: Dispatcher,
    middlewares: ServerMiddleware[] = [],
  ): Driver {
    return new Driver(connection, dispatcher, middlewares, new InMemoryOperationStore());
  }

  static withOperationStore(
    connection: ConnectionHandle,
    dispatcher: Dispatcher,
    store: OperationStore,
    middlewares: ServerMiddleware[] = [],
  ): Driver {
    return new Driver(connection, dispatcher, middlewares, store);
  }

  constructor(
    connection: ConnectionHandle,
    dispatcher: Dispatcher,
    middlewares: ServerMiddleware[] = [],
    store: OperationStore = new InMemoryOperationStore(),
  ) {
    this.connection = connection;
    this.dispatcher = dispatcher;
    this.middlewares = middlewares;
    this.operations = store;
  }

  withMiddleware(middleware: ServerMiddleware): Driver {
    return new Driver(this.connection, this.dispatcher, [...this.middlewares, middleware]);
  }

  async run(): Promise<void> {
    // r[impl rpc.session-setup]
    let pendingIncoming: Promise<IncomingCall | null> | null = null;
    let pendingCancel: Promise<bigint | null> | null = null;

    while (true) {
      await this.flushTaskQueue();

      if (!pendingIncoming) {
        pendingIncoming = this.connection.nextIncomingCall();
      }
      if (!pendingCancel) {
        pendingCancel = this.connection.nextIncomingCancel();
      }

      const wakeup = new Promise<"wakeup">((resolve) => {
        this.wakeupResolve = () => resolve("wakeup");
      });

      const race = await Promise.race([
        pendingIncoming.then((call) => ({ kind: "incoming" as const, call })),
        pendingCancel.then((requestId) => ({ kind: "cancel" as const, requestId })),
        wakeup.then((kind) => ({ kind })),
      ]);

      if (race.kind === "wakeup") {
        continue;
      }

      if (race.kind === "cancel") {
        pendingCancel = null;
        if (race.requestId !== null) {
          this.handleCancel(race.requestId);
        }
        continue;
      }

      pendingIncoming = null;
      if (!race.call) {
        break;
      }

      const task = this.handleCall(race.call).finally(() => {
        this.inFlight.delete(task);
        this.signalWakeup();
      });
      this.inFlight.add(task);
    }

    await Promise.allSettled([...this.inFlight]);
    await this.flushTaskQueue();
  }

  private signalWakeup(): void {
    const wakeup = this.wakeupResolve;
    this.wakeupResolve = null;
    wakeup?.();
  }

  private async flushTaskQueue(): Promise<void> {
    while (this.taskQueue.length > 0) {
      const message = this.taskQueue.shift()!;
      switch (message.kind) {
        case "data":
          await this.connection.sendChannelData(message.channelId, message.payload).catch((error) => {
            voxLogger()?.error("[vox:driver] failed to send channel data", error);
          });
          break;
        case "close":
          await this.connection.sendChannelClose(message.channelId).catch((error) => {
            voxLogger()?.error("[vox:driver] failed to send channel close", error);
          });
          break;
        case "grantCredit":
          await this.connection.sendChannelCredit(message.channelId, message.additional).catch((error) => {
            voxLogger()?.error("[vox:driver] failed to grant channel credit", error);
          });
          break;
        case "schema":
          await this.connection
            .sendSchemas(message.methodId, message.direction, message.schemas)
            .catch((error) => {
              voxLogger()?.error("[vox:driver] failed to send schema message", error);
            });
          break;
        case "response":
          await this.connection
            .sendResponse(
              message.requestId,
              message.payload,
              emptyMetadata(),
              [],
              message.schemas ? Array.from(message.schemas) : [],
            )
            .catch((error) => {
              voxLogger()?.error("[vox:driver] failed to send response", error);
            });
          break;
      }
    }
  }

  private async handleCall(incoming: IncomingCall): Promise<void> {
    // r[impl rpc.unknown-method]
    // r[impl rpc.response.one-per-request]
    const descriptor = this.dispatcher.getDescriptor();
    const method = descriptor.methods.get(incoming.methodId);
    voxLogger()?.debug(`[vox:driver] handleCall: methodId=${incoming.methodId} method=${method?.name ?? "UNKNOWN"}`);
    if (!method) {
      voxLogger()?.debug(`[vox:driver] unknown method, sending error response`);
      await this.connection.sendResponse(incoming.requestId, encodeUnknownMethod(descriptor));
      return;
    }

    const operationId = metadataOperationId(incoming.metadata);
    if (operationId !== undefined) {
      const admit = this.operations.admit(
        operationId,
        incoming.methodId,
        incoming.args,
        method.retry,
        incoming.requestId,
      );
      switch (admit.kind) {
        case "attached":
          return;
        case "replay": {
          // r[impl schema.interaction.retry]
          const schemas = this.connection.getSchemaSendTracker().prepareSchemas(
            method.id,
            "response",
            admit.response.responseSchemaClosure,
          );
          await this.connection.sendResponse(
            incoming.requestId,
            admit.response.payload,
            emptyMetadata(),
            [],
            schemas,
          );
          return;
        }
        case "conflict":
          await this.connection.sendResponse(incoming.requestId, encodeInvalidPayload(descriptor));
          return;
        case "indeterminate":
          await this.connection.sendResponse(incoming.requestId, encodeIndeterminate(descriptor));
          return;
        case "start":
          break;
      }
    }

    const context = new RequestContext(
      descriptor.service_name,
      method,
      incoming.metadata,
      new Extensions(),
    );
    const failClosedOnDrop = incoming.channels.length > 0 && !method.retry.idem;

    const taskSender: TaskSender = (message) => {
      this.taskQueue.push(message);
      this.signalWakeup();
    };

    const methodSchemas = descriptor.send_schemas[methodKey(method.id)];
    if (!methodSchemas) {
      voxLogger()?.error(`[vox:driver] no phon schemas for method ${method.id}`);
      await this.connection.sendResponse(incoming.requestId, encodeInvalidPayload(descriptor));
      return;
    }

    const call = new VoxCallImpl(
      method,
      incoming.requestId,
      taskSender,
      this.operations,
      operationId,
      this.connection.getSchemaSendTracker(),
      methodSchemas,
      descriptor.registry,
    );

    let outcome: ServerCallOutcome = { kind: "dropped" };

    try {
      await this.runPreHooks(context);
      const args = this.decodeArgs(
        descriptor,
        method,
        incoming,
        taskSender,
      );
      voxLogger()?.debug(`[vox:driver] dispatching ${method.name} with ${args.length} args`);
      await this.dispatcher.dispatch(context, method, args, call);
      voxLogger()?.debug(`[vox:driver] dispatch complete for ${method.name}, didReply=${call.didReply()}`);
      outcome = call.didReply() ? { kind: "replied" } : { kind: "dropped" };
      if (!call.didReply()) {
        if (operationId !== undefined) {
          const waiters = this.operations.failWithoutReply(operationId, incoming.requestId);
          for (const waiter of waiters) {
            taskSender({
              kind: "response",
              requestId: waiter,
              payload: method.retry.persist || failClosedOnDrop ? encodeIndeterminate(descriptor) : encodeCancelled(descriptor),
            });
          }
        } else if (method.retry.persist) {
          this.taskQueue.push({
            kind: "response",
            requestId: incoming.requestId,
            payload: encodeIndeterminate(descriptor),
          });
        } else {
          call.replyInternalError();
        }
      }
    } catch (error) {
      voxLogger()?.error(`[vox:driver] dispatch error for ${method.name}:`, error);
      if (!call.didReply()) {
        call.replyInternalError(error instanceof Error ? error.message : String(error));
      }
      outcome = { kind: "failed", error };
    }

    try {
      await this.runPostHooks(context, outcome);
    } finally {
      await this.flushTaskQueue();
    }
  }

  private handleCancel(requestId: bigint): void {
    const cancel = this.operations.cancel(requestId);
    switch (cancel.kind) {
      case "none":
        return;
      case "detach":
        return;
      case "release": {
        const descriptor = this.dispatcher.getDescriptor();
        for (const waiter of cancel.waiters) {
          this.taskQueue.push({
            kind: "response",
            requestId: waiter,
            payload: encodeCancelled(descriptor),
          });
        }
        this.signalWakeup();
        return;
      }
    }
  }

  private argsSchemaAdvertisingTaskSender(
    method: MethodDescriptor,
    methodSchemas: PhonMethodSchemas,
    taskSender: TaskSender,
  ): TaskSender {
    // r[impl schema.exchange.channels.tx-args]
    let advertised = false;
    return (message) => {
      if (!advertised && message.kind === "data") {
        advertised = true;
        const schemas = this.connection.getSchemaSendTracker().prepareSchemas(
          method.id,
          "args",
          methodSchemas.argsSchemaClosure,
        );
        if (schemas.length > 0) {
          taskSender({
            kind: "schema",
            methodId: method.id,
            direction: "args",
            schemas: new Uint8Array(schemas),
          });
        }
      }
      taskSender(message);
    };
  }

  private channelElementDeserializer(
    method: MethodDescriptor,
    channel: PhonChannelMeta,
    registry: Registry,
  ): (bytes: Uint8Array) => unknown {
    // r[impl schema.exchange.channels.rx-args]
    const role = channelElementRole(channel);
    return (bytes) => {
      const decoder = this.connection.getSchemaTracker().buildAuxiliaryDecoder(
        method.id,
        "args",
        role,
        channel.elementRoot,
        registry,
      );
      if (decoder) {
        return decoder(bytes) as unknown;
      }
      return decodeTyped(bytes, channel.elementRoot, channel.elementRoot, registry);
    };
  }

  private decodeArgs(
    descriptor: ServiceDescriptor,
    method: MethodDescriptor,
    incoming: IncomingCall,
    taskSender: TaskSender,
  ): unknown[] {
    // r[impl rpc.channel.binding]
    // r[impl rpc.channel.binding.callee-args.rx]
    // r[impl rpc.channel.binding.callee-args.tx]
    const ms = descriptor.send_schemas[methodKey(method.id)];
    if (!ms) {
      throw new Error(`no phon schemas for method ${method.id}`);
    }
    const registry = descriptor.registry;

    // Decode the args tuple, reconciling the peer's writer closure (recorded by
    // the session in the `schemas:` field) against our `argsRoot` reader. A 0-arg
    // method carries no bytes. Falls back to writer==reader when nothing was sent.
    let values: unknown[] = [];
    if (incoming.args.length > 0) {
      const decoder =
        this.connection.getSchemaTracker().buildDecoder(method.id, "args", ms.argsRoot, registry) ??
        ((bytes: Uint8Array) =>
          decodeTyped(bytes, ms.argsRoot, ms.argsRoot, registry));
      values = decoder(incoming.args) as unknown[];
    }

    if (ms.channels.length === 0) {
      return values;
    }

    // Bind each server-side `Tx`/`Rx` from `RequestCall.channels`. The decoded
    // arg at a channel position is the 4-byte LE wire index into that list
    // (`r[rpc.channel.payload-encoding]`); resolve it to a `ChannelId` and replace
    // the slot with a runtime handle whose per-item codec is keyed on the element.
    const channelRegistry = this.connection.getChannelRegistry();
    const creditOut = this.connection.peerSettings.initial_channel_credit ?? DEFAULT_INITIAL_CREDIT;
    const creditIn = this.connection.localSettings.initial_channel_credit ?? DEFAULT_INITIAL_CREDIT;
    const serverTxTaskSender = ms.channels.some((ch) => ch.direction === "tx")
      ? this.argsSchemaAdvertisingTaskSender(method, ms, taskSender)
      : taskSender;
    for (const ch of ms.channels) {
      const wireIndex = readU32LE(values[ch.index] as Uint8Array);
      const channelId = incoming.channels[wireIndex];
      if (channelId === undefined) {
        throw new Error(`channel wire index ${wireIndex} out of range (${incoming.channels.length})`);
      }
      if (ch.direction === "tx") {
        // The handler holds a `Tx` and SENDS to the caller.
        values[ch.index] = createServerTx(
          channelId,
          serverTxTaskSender,
          channelRegistry,
          creditOut,
          (value: unknown) => encodeTyped(value as never, ch.elementRoot, registry),
        );
      } else {
        // The handler holds an `Rx` and RECEIVES from the caller.
        const receiver = channelRegistry.registerIncoming(channelId, creditIn);
        values[ch.index] = createServerRx(
          channelId,
          receiver,
          this.channelElementDeserializer(method, ch, registry),
        );
      }
    }

    return values;
  }

  private async runPreHooks(context: RequestContext): Promise<void> {
    for (const middleware of this.middlewares) {
      await middleware.pre?.(context);
    }
  }

  private async runPostHooks(
    context: RequestContext,
    outcome: ServerCallOutcome,
  ): Promise<void> {
    for (let i = this.middlewares.length - 1; i >= 0; i--) {
      await this.middlewares[i]?.post?.(context, outcome);
    }
  }
}

// Protocol-error responses are `Result<T, VoxError<E>>::Err(VoxError::…)`. The
// `Err` payload (UnknownMethod / Cancelled / Indeterminate / InvalidPayload) is
// independent of the method's `T`/`E`, so any method's `responseRoot` encodes it;
// the caller decodes against its own response root (no schema is advertised).
function encodeVoxError(
  descriptor: ServiceDescriptor,
  err: { tag: string; value?: unknown },
): Uint8Array {
  for (const ms of Object.values(descriptor.send_schemas)) {
    return encodeTyped({ tag: "Err", value: err } as never, ms.responseRoot, descriptor.registry);
  }
  throw new Error("service has no methods to derive a response root");
}

function encodeUnknownMethod(descriptor: ServiceDescriptor): Uint8Array {
  return encodeVoxError(descriptor, { tag: "UnknownMethod" });
}

function encodeInvalidPayload(descriptor: ServiceDescriptor): Uint8Array {
  return encodeVoxError(descriptor, { tag: "InvalidPayload", value: "invalid payload" });
}

function encodeCancelled(descriptor: ServiceDescriptor): Uint8Array {
  return encodeVoxError(descriptor, { tag: "Cancelled" });
}

function encodeIndeterminate(descriptor: ServiceDescriptor): Uint8Array {
  return encodeVoxError(descriptor, { tag: "Indeterminate" });
}
