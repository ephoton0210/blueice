// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A thin wrapper over the official MCP TypeScript/JS SDK
// (`@modelcontextprotocol/sdk`) for talking to `blueice-mcp-server` --
// reused rather than hand-rolling JSON-RPC-over-stdio, the same
// "solved infrastructure" reasoning the Rust side used `rmcp` for
// (`backend/mcp-server`). Exposes exactly the three tool calls this
// harness needs (`navigate`, `get_dom`, `screenshot`), not a general
// MCP-tool-calling facade.

import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const __dirname = dirname(fileURLToPath(import.meta.url));

/**
 * `blueice-mcp-server` lands in the Cargo workspace's shared `target/`
 * dir, same as every other workspace binary. Defaults to the debug
 * profile; override with `BLUEICE_MCP_SERVER_BIN` for a release build
 * or a non-default target directory.
 */
function findMcpServerBinary() {
  if (process.env.BLUEICE_MCP_SERVER_BIN) return process.env.BLUEICE_MCP_SERVER_BIN;
  const repoRoot = join(__dirname, "../..");
  const path = join(repoRoot, "target/debug/blueice-mcp-server");
  if (!existsSync(path)) {
    throw new Error(`blueice-mcp-server binary not found at ${path} -- run \`cargo build --workspace\` first, or set BLUEICE_MCP_SERVER_BIN`);
  }
  return path;
}

export class BlueIceClient {
  #client;
  #transport;

  async connect() {
    this.#transport = new StdioClientTransport({ command: findMcpServerBinary() });
    this.#client = new Client({ name: "blueice-differential-testing", version: "0.1.0" });
    await this.#client.connect(this.#transport);
  }

  async #callTool(name, args) {
    const result = await this.#client.callTool({ name, arguments: args ?? {} });
    if (result.isError) {
      throw new Error(`blueice-mcp-server tool "${name}" failed: ${JSON.stringify(result.content)}`);
    }
    return result;
  }

  /** Navigates BlueIce to `url`, returning `{ error, snapshot }` (see `blueice_mcp_server::ToolOutcome`). */
  async navigate(url) {
    const result = await this.#callTool("navigate", { url });
    return JSON.parse(result.content[0].text);
  }

  /** The full, unfiltered DOM tree dump (`blueice_dom::dump`'s format). */
  async getDom() {
    const result = await this.#callTool("get_dom");
    return result.content[0].text;
  }

  /** A PNG screenshot of the most recently rendered frame, as a `Buffer`. */
  async screenshotPng() {
    const result = await this.#callTool("screenshot");
    const image = result.content.find((c) => c.type === "image");
    if (!image) throw new Error('screenshot tool did not return an "image" content block');
    return Buffer.from(image.data, "base64");
  }

  async close() {
    await this.#client.close();
  }
}
