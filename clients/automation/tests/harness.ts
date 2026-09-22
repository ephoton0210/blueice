// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Shared subprocess-test scaffolding for this package's own test
// suite -- spawns real `blueice-core` + `blueice-automation` binaries,
// wired to a real local HTTP fixture and a Node-native
// `blueice_ipc::gatekeeper` "always clears" stub, and completes the
// frontend handshake + one `Navigate` so the default tab has real,
// inspectable content by the time a test gets its `Harness` back. Not
// part of `@blueice/automation` itself -- both `client.test.ts` and
// `devtools_inspect_cli.test.ts` import this rather than duplicating
// it.

import { spawn } from "node:child_process";
import { createServer as createNetServer, createConnection, type Socket } from "node:net";
import { createServer as createHttpServer } from "node:http";
import { existsSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(fileURLToPath(import.meta.url), "..", "..", "..", "..");

export function binaryPath(name: string): string {
  const path = join(repoRoot, "target", "debug", name);
  if (!existsSync(path)) {
    throw new Error(`${path} not found -- run \`cargo build --workspace\` first`);
  }
  return path;
}

function uniquePath(label: string): string {
  return join(tmpdir(), `bic-ts-${label}-${process.pid}.sock`);
}

function waitFor(path: string, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const tick = () => {
      if (existsSync(path)) return resolve();
      if (Date.now() > deadline) return reject(new Error(`${path} never appeared`));
      setTimeout(tick, 20);
    };
    tick();
  });
}

/** This file's own copy of the length-prefixed-JSON frame shape, for
 * the two raw wire protocols this harness drives that aren't
 * `@blueice/automation`'s own concern: the external `ClientMessage`
 * frontend protocol (just enough to `Hello` + `Navigate` the default
 * tab) and `blueice_ipc::gatekeeper` (the stub server below). */
function writeFrame(socket: Socket, value: unknown): void {
  const body = Buffer.from(JSON.stringify(value), "utf8");
  const length = Buffer.alloc(4);
  length.writeUInt32LE(body.length, 0);
  socket.write(Buffer.concat([length, body]));
}

function readFrame(socket: Socket): Promise<unknown> {
  return new Promise((resolve, reject) => {
    let buffered = Buffer.alloc(0);
    let expected: number | null = null;
    const onData = (chunk: Buffer) => {
      buffered = Buffer.concat([buffered, chunk]);
      if (expected === null && buffered.length >= 4) {
        expected = buffered.readUInt32LE(0);
        buffered = buffered.subarray(4);
      }
      if (expected !== null && buffered.length >= expected) {
        cleanup();
        resolve(JSON.parse(buffered.subarray(0, expected).toString("utf8")));
      }
    };
    const onError = (error: Error) => {
      cleanup();
      reject(error);
    };
    function cleanup() {
      socket.off("data", onData);
      socket.off("error", onError);
    }
    socket.on("data", onData);
    socket.on("error", onError);
  });
}

/** A Unix-socket server that always clears every `blueice_ipc::gatekeeper`
 * request it receives -- a Node-native stand-in for the real
 * `blueice-ai-gatekeeper` binary's own minimal-slice "always clears"
 * behavior (`blueice_ai_gatekeeper::handle_one_check`), used here only
 * so this harness doesn't depend on that binary's uncustomizable
 * well-known default socket path. */
function startClearingGatekeeperStub(path: string): () => void {
  const server = createNetServer((socket) => {
    readFrame(socket)
      .then(() => writeFrame(socket, "Cleared"))
      .catch(() => {})
      .finally(() => socket.end());
  });
  server.listen(path);
  return () => server.close();
}

function startFixtureHttpServer(): Promise<{ url: string; close: () => void }> {
  return new Promise((resolve) => {
    const server = createHttpServer((_req, res) => {
      const body = '<main><button id="save">Save</button></main>';
      res.writeHead(200, { "Content-Type": "text/html", "Content-Length": Buffer.byteLength(body) });
      res.end(body);
    });
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (address === null || typeof address === "string") {
        throw new Error("expected a real TCP address from the fixture HTTP server");
      }
      resolve({ url: `http://127.0.0.1:${address.port}`, close: () => server.close() });
    });
  });
}

export interface Harness {
  coreSocket: string;
  automationSocket: string;
  token: string;
  tokenFile: string;
  cleanup: () => void;
}

export async function spawnCoreAndAdapter(label: string): Promise<Harness> {
  const coreSocket = uniquePath(`${label}-c`);
  const coreAutomationSocket = uniquePath(`${label}-ca`);
  const adapterSocket = uniquePath(`${label}-a`);
  const tokenFile = uniquePath(`${label}-t`);
  const gatekeeperSocket = uniquePath(`${label}-g`);
  const frameDir = join(tmpdir(), `bic-ts-frames-${label}-${process.pid}`);

  const stopGatekeeper = startClearingGatekeeperStub(gatekeeperSocket);
  const fixture = await startFixtureHttpServer();

  const core = spawn(binaryPath("blueice-core"), [
    "--socket",
    coreSocket,
    "--automation-socket",
    coreAutomationSocket,
    "--gatekeeper-socket",
    gatekeeperSocket,
    "--frame-dir",
    frameDir,
  ]);
  const adapter = spawn(binaryPath("blueice-automation"), [
    "--socket",
    adapterSocket,
    "--core-socket",
    coreAutomationSocket,
    "--token-file",
    tokenFile,
  ]);

  await waitFor(coreSocket, 5000);
  await waitFor(coreAutomationSocket, 5000);
  await waitFor(adapterSocket, 5000);
  await waitFor(tokenFile, 5000);
  const token = readFileSync(tokenFile, "utf8");

  // Unblocks `core`'s dispatch loop (and so automation-request
  // draining) by completing the frontend handshake, then navigates the
  // default tab to real content -- the automation protocol itself has
  // no `Navigate` command yet (a documented Slice 1 limitation; see
  // `blueice_ipc::automation`'s module docs), so this is the only way
  // to get inspectable content into a tab for this harness's callers.
  const frontend = createConnection(coreSocket);
  await new Promise<void>((resolve, reject) => {
    frontend.once("connect", () => resolve());
    frontend.once("error", reject);
  });
  writeFrame(frontend, { message: { Hello: { protocol_version: 1 } } });
  await readFrame(frontend); // Hello ack
  writeFrame(frontend, { message: { Navigate: { url: fixture.url } } });
  await readFrame(frontend); // Navigated
  await readFrame(frontend); // FrameReady

  function cleanup() {
    frontend.end();
    core.kill();
    adapter.kill();
    stopGatekeeper();
    fixture.close();
    rmSync(frameDir, { recursive: true, force: true });
  }

  return { coreSocket, automationSocket: adapterSocket, token, tokenFile, cleanup };
}
