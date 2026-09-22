// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Owns the local process prerequisites of a clean Phase 15 run. The MCP
// server attaches to a launcher when one is available, but a clean checkout
// otherwise needs the always-resident gatekeeper (fail-closed navigation) and
// launcher (which starts core together with its BlueJS sibling). Reuse live
// user services when present; only terminate a process this harness started.

import { existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { createConnection } from "node:net";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(__dirname, "../..");
const START_TIMEOUT_MS = 5_000;

function defaultSocketDirectory() {
  return process.env.XDG_RUNTIME_DIR
    ? join(process.env.XDG_RUNTIME_DIR, "blueice")
    : join(tmpdir(), `blueice-${process.getuid()}`);
}

function defaultPaths() {
  const directory = defaultSocketDirectory();
  return {
    gatekeeper: join(directory, "ai-gatekeeper.sock"),
    launcher: join(directory, "core.sock"),
  };
}

function binary(name, override) {
  const path = process.env[override] ?? join(repoRoot, "target/debug", name);
  if (!existsSync(path)) {
    throw new Error(`${name} binary not found at ${path} -- run \`cargo build --workspace --all-targets\` first, or set ${override}`);
  }
  return path;
}

function socketIsReachable(path) {
  return new Promise((resolve) => {
    const socket = createConnection(path);
    const done = (reachable) => {
      socket.removeAllListeners();
      socket.destroy();
      resolve(reachable);
    };
    socket.once("connect", () => done(true));
    socket.once("error", () => done(false));
  });
}

async function waitForSocket(path, processName) {
  const deadline = Date.now() + START_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (await socketIsReachable(path)) return;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`${processName} did not accept connections at ${path} within ${START_TIMEOUT_MS}ms`);
}

async function start(path, processName, socketPath) {
  // A detached process starts its own process group. On cleanup that lets the
  // harness stop launcher's core and BlueJS descendants together, rather than
  // orphaning either of them after one differential invocation.
  const child = spawn(path, [], { detached: true, stdio: ["ignore", "inherit", "inherit"] });
  try {
    await waitForSocket(socketPath, processName);
    return child;
  } catch (error) {
    if (child.pid) {
      try { process.kill(-child.pid, "SIGTERM"); } catch { /* it already exited */ }
    }
    throw error;
  }
}

async function stopProcessGroup(child) {
  if (!child?.pid || child.exitCode !== null) return;
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
  await new Promise((resolve) => child.once("exit", resolve));
}

export class BlueIceRuntime {
  #gatekeeper;
  #launcher;

  /** Starts only the services absent from the current user's BlueIce session. */
  static async ensureReady() {
    const runtime = new BlueIceRuntime();
    const paths = defaultPaths();
    try {
      if (!await socketIsReachable(paths.gatekeeper)) {
        runtime.#gatekeeper = await start(binary("blueice-ai-gatekeeper", "BLUEICE_GATEKEEPER_BIN"), "blueice-ai-gatekeeper", paths.gatekeeper);
      }
      if (!await socketIsReachable(paths.launcher)) {
        runtime.#launcher = await start(binary("blueice-launcher", "BLUEICE_LAUNCHER_BIN"), "blueice-launcher", paths.launcher);
      }
      return runtime;
    } catch (error) {
      await runtime.close();
      throw error;
    }
  }

  /** Leaves pre-existing user services alone; reaps only this run's children. */
  async close() {
    await stopProcessGroup(this.#launcher);
    await stopProcessGroup(this.#gatekeeper);
  }
}
