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

// Every `blueice-mcp-server` tool result carrying page-derived content
// (`navigate`'s snapshot, `get_dom`'s dump) is prefixed with an
// untrusted-content warning ahead of a fixed marker line
// (`wrap_untrusted_page_content` in `backend/mcp-server/src/lib.rs`)
// before the real content, verbatim, with no closing marker. Strip
// that prefix here, once, rather than in each caller.
const UNTRUSTED_CONTENT_MARKER = "--- BEGIN UNTRUSTED PAGE CONTENT ---";

function unwrapUntrustedContent(text) {
  const index = text.indexOf(UNTRUSTED_CONTENT_MARKER);
  if (index === -1) return text;
  return text.slice(index + UNTRUSTED_CONTENT_MARKER.length).replace(/^\n/, "");
}

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
    return JSON.parse(unwrapUntrustedContent(result.content[0].text));
  }

  /** The full, unfiltered DOM tree dump (`blueice_dom::dump`'s format). */
  async getDom() {
    const result = await this.#callTool("get_dom");
    return unwrapUntrustedContent(result.content[0].text);
  }

  /** The current page's AI-facing snapshot, without performing any action. */
  async getPageRepresentation() {
    const result = await this.#callTool("get_page_representation");
    return JSON.parse(unwrapUntrustedContent(result.content[0].text));
  }

  /** A PNG screenshot of the most recently rendered frame, as a `Buffer`. */
  async screenshotPng() {
    const result = await this.#callTool("screenshot");
    const image = result.content.find((c) => c.type === "image");
    if (!image) throw new Error('screenshot tool did not return an "image" content block');
    return Buffer.from(image.data, "base64");
  }

  /**
   * Screenshots the page `navigate` just navigated to, waiting for its
   * frame to actually be ready first. `navigate` itself
   * (`phase-7-local-ai/PLAN.md`'s non-blocking dispatch: gatekeeper
   * review and the fetch run on a background thread) can return before
   * that background pipeline's own `FrameReady` reaches this
   * connection, and nothing re-reads the socket to notice a `FrameReady`
   * that arrives afterward until another call does -- so a bare
   * `screenshotPng()` right after `navigate` can spuriously fail with
   * "no frame has been rendered yet". Retries `get_page_representation`
   * (a real round trip over the same connection, so it drains any
   * `FrameReady` sitting unread on the wire) between attempts.
   */
  async screenshotPngWhenReady(timeoutMs = 5000, pollIntervalMs = 100) {
    const deadline = Date.now() + timeoutMs;
    let lastError;
    while (Date.now() < deadline) {
      try {
        return await this.screenshotPng();
      } catch (error) {
        lastError = error;
        await this.getPageRepresentation();
        await new Promise((resolve) => setTimeout(resolve, pollIntervalMs));
      }
    }
    throw lastError ?? new Error("screenshotPngWhenReady timed out with no attempt made");
  }

  async close() {
    await this.#client.close();
  }
}
