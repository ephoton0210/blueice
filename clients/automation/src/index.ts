// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// `@blueice/automation`: the TypeScript browser/context/page/locator
// client `phase-17-automation-devtools-and-ajax/PLAN.md`'s "Playwright-
// shaped workflow" decision and Slice 1 item 4 ask for, over the real
// `blueice_ipc::automation` wire protocol -- connecting through a real
// `blueice-automation` adapter process (never directly to `core`'s own
// automation socket, which has no authentication of its own; see that
// crate's module docs). This is a minimal first slice matching the
// same deliberate scope every other Phase 17 Slice 1 piece uses: the
// four classes below cover the representative commands `core`'s
// automation service actually implements today (lifecycle, DOM/AX/
// screenshot inspection, locator resolution, a same-page click) --
// not the full eventual Playwright-shaped surface (`waitFor*`,
// `route`, multiple selector engines, ...), and `Locator.click` is
// itself scoped to `AutomationRequest::Click`'s own current
// limitation (a same-page interaction only -- see
// `blueice_engine::automation_service`'s doc comment on `Click` for
// why a link's href-follow case is deliberately deferred).
//
// Runs as plain `.ts` under Node's native type-stripping (Node 24+,
// no `--experimental-strip-types` flag needed by the version pinned
// in this repo's CI) -- no bundler, no `tsc`, no build step for this
// minimal slice; `tests/` is run the same way via `node --test`.

import { createConnection, type Socket } from "node:net";

/** Mirrors `blueice_ipc::automation::Capability`'s seven variants. */
export type Capability =
  | "Lifecycle"
  | "Inspection"
  | "LocatorsAndWaiting"
  | "Input"
  | "ScriptRuntime"
  | "NetworkAndTracing"
  | "ApiWorkspace";

/**
 * Mirrors `blueice_ipc::automation::ControllerLease` -- opaque from a
 * client's own perspective, just a value to hold and pass back to
 * `Browser.releaseControllerLease`.
 */
export type ControllerLease = string;

/**
 * A loose mirror of `blueice_ipc::AiSnapshot` -- `nodes` is kept
 * `unknown[]` rather than a full field-by-field `AiNode` type, since
 * duplicating that schema here would just drift out of sync with the
 * Rust source of truth as it grows; callers that need typed node
 * access should narrow `nodes` themselves.
 */
export interface AccessibilityTree {
  generation: number;
  tab_id: number;
  url: string | null;
  scroll_y: number;
  nodes: unknown[];
}

/**
 * Thrown for any `AutomationReply::Error(...)` this client receives --
 * `detail` is the raw decoded `AutomationError` value (an object for a
 * variant with fields, e.g. `{ CapabilityNotGranted: { capability:
 * "Input" } }`, or a bare string for a unit variant like
 * `"NoControllerLease"`), so a caller can branch on it without this
 * client re-declaring the whole error taxonomy as TypeScript types.
 */
export class AutomationError extends Error {
  readonly detail: unknown;

  constructor(detail: unknown) {
    super(`blueice-automation error: ${JSON.stringify(detail)}`);
    this.name = "AutomationError";
    this.detail = detail;
  }
}

/**
 * Buffers a socket's incoming bytes and hands out exact-length reads
 * as they become available -- the length-prefixed framing this
 * protocol uses needs "give me exactly N bytes, whenever that many
 * have arrived," which a single persistent `data` listener plus a
 * FIFO queue implements without re-attaching listeners per read.
 *
 * Also tracks the socket's `close`/`error` events and rejects every
 * still-pending (and any future) read with them -- without this, a
 * peer that drops the connection instead of replying (e.g.
 * `blueice-automation` silently closing on a wrong token; see that
 * crate's `serve_connection` docs) would leave a caller's `readExact`
 * promise pending forever instead of failing.
 */
class FrameReader {
  #buffered = Buffer.alloc(0);
  #queue: { n: number; resolve: (b: Buffer) => void; reject: (e: Error) => void }[] = [];
  #closedError: Error | null = null;

  constructor(socket: Socket) {
    socket.on("data", (chunk: Buffer) => {
      this.#buffered = Buffer.concat([this.#buffered, chunk]);
      this.#drain();
    });
    const fail = (error: Error) => this.#fail(error);
    socket.on("close", () => fail(new Error("blueice-automation connection closed")));
    socket.on("error", fail);
  }

  #fail(error: Error): void {
    this.#closedError ??= error;
    while (this.#queue.length > 0) {
      this.#queue.shift()!.reject(error);
    }
  }

  #drain(): void {
    while (this.#queue.length > 0 && this.#buffered.length >= this.#queue[0]!.n) {
      const { n, resolve } = this.#queue.shift()!;
      resolve(this.#buffered.subarray(0, n));
      this.#buffered = this.#buffered.subarray(n);
    }
  }

  readExact(n: number): Promise<Buffer> {
    if (this.#closedError) {
      return Promise.reject(this.#closedError);
    }
    return new Promise((resolve, reject) => {
      this.#queue.push({ n, resolve, reject });
      this.#drain();
    });
  }
}

/** This crate's own copy of `blueice_ipc`'s length-prefixed-JSON frame
 * shape (a `u32` little-endian byte length, then that many raw UTF-8
 * JSON bytes) -- see `blueice-automation`'s own `lib.rs` module docs
 * for why every language-side reimplementation of this wire format
 * keeps landing on "just re-declare the tiny framing function" rather
 * than exporting `blueice_ipc`'s private one. */
async function readFrame(reader: FrameReader): Promise<unknown> {
  const lengthBytes = await reader.readExact(4);
  const length = lengthBytes.readUInt32LE(0);
  const body = await reader.readExact(length);
  return JSON.parse(body.toString("utf8"));
}

function writeFrame(socket: Socket, value: unknown): void {
  const body = Buffer.from(JSON.stringify(value), "utf8");
  const length = Buffer.alloc(4);
  length.writeUInt32LE(body.length, 0);
  socket.write(Buffer.concat([length, body]));
}

export interface ConnectOptions {
  /** Path to a real `blueice-automation` adapter's own socket -- not `core`'s automation socket directly. */
  socketPath: string;
  /** The token `blueice-automation` wrote to its token file (see that crate's `default_token_path`). */
  token: string;
  clientName?: string;
  /** Defaults to every capability this minimal slice's automation service actually implements. */
  capabilities?: Capability[];
}

const DEFAULT_CAPABILITIES: Capability[] = [
  "Lifecycle",
  "Inspection",
  "LocatorsAndWaiting",
  "Input",
];

/**
 * The one connection every `Browser` wraps: writes the adapter's
 * token preamble, then speaks raw `blueice_ipc::automation` JSON
 * frames -- one request in flight at a time, matching
 * `AutomationRequestSender::request`'s own blocking request/reply
 * shape on the Rust side (this protocol carries no `request_id` to
 * pipeline multiple in-flight requests over one connection).
 */
class AutomationChannel {
  #socket: Socket;
  #reader: FrameReader;

  private constructor(socket: Socket) {
    this.#socket = socket;
    this.#reader = new FrameReader(socket);
  }

  static async open(socketPath: string, token: string): Promise<AutomationChannel> {
    const socket = await new Promise<Socket>((resolve, reject) => {
      const s = createConnection(socketPath, () => resolve(s));
      s.once("error", reject);
    });
    const channel = new AutomationChannel(socket);
    writeFrame(socket, { token });
    return channel;
  }

  async request(req: unknown): Promise<Record<string, unknown> | string> {
    writeFrame(this.#socket, req);
    const reply = await readFrame(this.#reader);
    if (reply !== null && typeof reply === "object" && "Error" in (reply as object)) {
      throw new AutomationError((reply as { Error: unknown }).Error);
    }
    return reply as Record<string, unknown> | string;
  }

  close(): void {
    this.#socket.end();
  }
}

/** The top-level handle a script holds -- one per adapter connection. */
export class Browser {
  #channel: AutomationChannel;
  readonly grantedCapabilities: readonly Capability[];

  private constructor(channel: AutomationChannel, granted: Capability[]) {
    this.#channel = channel;
    this.grantedCapabilities = granted;
  }

  static async connect(options: ConnectOptions): Promise<Browser> {
    const channel = await AutomationChannel.open(options.socketPath, options.token);
    const requested = options.capabilities ?? DEFAULT_CAPABILITIES;
    const reply = await channel.request({
      Hello: {
        client_name: options.clientName ?? "@blueice/automation",
        requested_capabilities: requested,
      },
    });
    const helloAck = (reply as { HelloAck: { granted_capabilities: Capability[] } }).HelloAck;
    return new Browser(channel, helloAck.granted_capabilities);
  }

  /** Creates a fresh, isolated browser context -- `TabManager`'s `BrowserContextId -> TabId -> Page` layer. */
  async newContext(): Promise<BrowserContext> {
    const reply = await this.#channel.request("CreateContext");
    const { context_id } = (reply as { ContextCreated: { context_id: number } }).ContextCreated;
    return new BrowserContext(this.#channel, context_id);
  }

  /**
   * Wraps an already-known tab ID -- e.g. the default tab every fresh
   * `core` process starts with (`tab_id` `1`), or one learned some
   * other way -- as a `Page`, with no server round trip of its own.
   * This protocol has no `ListTabs`/enumeration command of its own
   * (unlike the external `ClientMessage` protocol's `ListTabs`), so
   * attaching to a tab this way is the only route to one this client
   * didn't itself open via `BrowserContext.newPage`; validity is
   * checked lazily by whichever request the returned `Page` first
   * makes, the same way every other `tab_id` on this wire already
   * works.
   */
  page(tabId: number): Page {
    return new Page(this.#channel, tabId);
  }

  /** Acquires the exclusive controller lease every mutating command (e.g. `Locator.click`) requires. */
  async acquireControllerLease(): Promise<ControllerLease> {
    const reply = await this.#channel.request("AcquireControllerLease");
    return (reply as { ControllerLeaseGranted: { lease: ControllerLease } })
      .ControllerLeaseGranted.lease;
  }

  async releaseControllerLease(lease: ControllerLease): Promise<void> {
    await this.#channel.request({ ReleaseControllerLease: { lease } });
  }

  close(): void {
    this.#channel.close();
  }
}

export class BrowserContext {
  readonly contextId: number;
  #channel: AutomationChannel;

  /** @internal -- construct via `Browser.newContext`. */
  constructor(channel: AutomationChannel, contextId: number) {
    this.#channel = channel;
    this.contextId = contextId;
  }

  /** Opens a new, blank page under this context. */
  async newPage(): Promise<Page> {
    const reply = await this.#channel.request({ OpenTab: { context_id: this.contextId } });
    const { tab_id } = (reply as { TabOpened: { tab_id: number } }).TabOpened;
    return new Page(this.#channel, tab_id);
  }

  /** Closes this context and every page under it. */
  async close(): Promise<void> {
    await this.#channel.request({ CloseContext: { context_id: this.contextId } });
  }
}

export class Page {
  readonly tabId: number;
  #channel: AutomationChannel;

  /** @internal -- construct via `BrowserContext.newPage`. */
  constructor(channel: AutomationChannel, tabId: number) {
    this.#channel = channel;
    this.tabId = tabId;
  }

  /** The full DOM tree dump (`blueice_dom::dump`'s canonical text format). */
  async dom(): Promise<string> {
    const reply = await this.#channel.request({ GetDom: { tab_id: this.tabId } });
    return (reply as { Dom: { dump: string } }).Dom.dump;
  }

  /** The accessibility-tree-shaped representation a human and an AI perceive from the same render pass. */
  async accessibilityTree(): Promise<AccessibilityTree> {
    const reply = await this.#channel.request({ GetAccessibilityTree: { tab_id: this.tabId } });
    return (reply as { AccessibilityTree: { snapshot: AccessibilityTree } }).AccessibilityTree
      .snapshot;
  }

  /** A PNG screenshot of this tab's most recently rendered frame. */
  async screenshot(): Promise<Buffer> {
    const reply = await this.#channel.request({ Screenshot: { tab_id: this.tabId } });
    const { png_base64 } = (reply as { Screenshot: { png_base64: string } }).Screenshot;
    return Buffer.from(png_base64, "base64");
  }

  /** Resolves `cssSelector` against this page's current document (see `Locator`). */
  locator(cssSelector: string): Locator {
    return new Locator(this.#channel, this.tabId, cssSelector);
  }
}

export class Locator {
  #channel: AutomationChannel;
  #tabId: number;
  #cssSelector: string;

  /** @internal -- construct via `Page.locator`. */
  constructor(channel: AutomationChannel, tabId: number, cssSelector: string) {
    this.#channel = channel;
    this.#tabId = tabId;
    this.#cssSelector = cssSelector;
  }

  /** Every currently-matching element's stable node ID, in document order. */
  async resolve(): Promise<number[]> {
    const reply = await this.#channel.request({
      ResolveLocator: { tab_id: this.#tabId, css_selector: this.#cssSelector },
    });
    return (reply as { LocatorResolved: { node_ids: number[] } }).LocatorResolved.node_ids;
  }

  /**
   * Clicks the first matching element. Requires a held controller
   * lease (`Browser.acquireControllerLease`) and, per
   * `AutomationRequest::Click`'s own current scope, only actually
   * does anything for a non-navigating control -- a link's href-follow
   * case is a documented later-slice limitation, not something this
   * client can route around.
   */
  async click(): Promise<void> {
    const [nodeId] = await this.resolve();
    if (nodeId === undefined) {
      throw new Error(`locator ${JSON.stringify(this.#cssSelector)} matched no elements`);
    }
    await this.#channel.request({ Click: { tab_id: this.#tabId, node_id: nodeId } });
  }
}
