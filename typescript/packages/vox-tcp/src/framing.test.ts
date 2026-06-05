import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { LocalLink, LocalLinkAcceptor } from "./transport.ts";

const cleanups: Array<() => Promise<void>> = [];

afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) {
    await cleanup();
  }
});

async function makeLocalEndpoint(): Promise<{ addr: string; cleanup: () => Promise<void> }> {
  if (process.platform === "win32") {
    return {
      addr: `\\\\.\\pipe\\vox-framing-${randomUUID()}`,
      cleanup: async () => {},
    };
  }

  const dir = await mkdtemp(join(tmpdir(), "vox-framing-"));
  return {
    addr: join(dir, "sock"),
    cleanup: () => rm(dir, { recursive: true, force: true }),
  };
}

describe("LengthPrefixedFramed", () => {
  // r[verify rpc.transport.stream.cancel-safe-recv]
  it("keeps partial frames in transport-owned state after recv timeout", async () => {
    const endpoint = await makeLocalEndpoint();
    cleanups.push(endpoint.cleanup);
    const acceptor = await LocalLinkAcceptor.bind(endpoint.addr);

    try {
      const accepted = acceptor.nextLink();
      const client = await LocalLink.connect(endpoint.addr);
      const server = (await accepted).link;
      const socket = server.getSocket();
      const frame = Buffer.alloc(7);
      frame.writeUInt32LE(3, 0);
      frame.set(Uint8Array.of(7, 8, 9), 4);

      socket.write(frame.subarray(0, 5));
      await expect(client.recvTimeout(5)).resolves.toBeNull();
      socket.write(frame.subarray(5));

      await expect(client.recv()).resolves.toEqual(Uint8Array.of(7, 8, 9));

      client.close();
      server.close();
    } finally {
      await acceptor.close();
    }
  });
});
