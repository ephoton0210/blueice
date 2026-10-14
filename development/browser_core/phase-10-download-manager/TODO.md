# TODO — BlueIce download manager (Phase 10, first slice)

[← Back to Phase 10 plan](PLAN.md)

> **Status: decisions confirmed in review; design recorded in [`PLAN.md`](PLAN.md)'s "Wiring design (resolved 2026-09-20)", which is the authoritative record if the two ever differ. M0 (design), M1 (`blueice-ipc` contract), M2 (`blueice-net` transfer engine), M3 (the `downloads` process), M4 (the MCP tools), M5 (`about:downloads`) and M6 (wrap-up) are done: the first slice is feature-complete. A post-slice audit (2026-09-21, [§3](#3-post-slice-audit-follow-ups-2026-09-21)) found defects the milestone tests did not catch, including four P1 items (a 0-byte download that never completes, a transfer that hangs after every connection stalls, destination collisions, an unusable default socket path on macOS). Those follow-ups are now complete.**
> Branch: `feature/downloads-first-slice`.
> Design inputs: [`PLAN.md`](PLAN.md) (this phase), [`../phase-7-local-ai/PLAN.md`](../phase-7-local-ai/PLAN.md), [`../phase-12-mcp-server/PLAN.md`](../phase-12-mcp-server/PLAN.md), [`../research/safe-browsing-enforcement.md`](../research/safe-browsing-enforcement.md), [`../research/multi-process-memory.md`](../research/multi-process-memory.md).

## 0. Goal and scope

**Goal**: match the core capabilities of Free Download Manager, and make every transfer fully observable to an AI agent over MCP.

1. **Segmented, resumable, accelerated downloads**: multi-connection segmented transfer, dynamic re-splitting, pause/resume, retries, single-stream fallback.
2. **Visualization**: a built-in `about:downloads` page rendered by BlueIce's own engine (decided), so a human and an AI agent see the same render pass.
3. **AI observability**: MCP tools report progress, speed, ETA, per-segment state, retries, errors, and what the safety gatekeeper decided.

**Scope**: the engine, the `downloads` process, the MCP tools, and the visualization, delivered together (decided).

**Explicitly out of scope for this slice**: a scheduler, speed limits, multi-mirror/multi-source downloads, FTP/SFTP (Phase 11; the shared `TransferBackend` trait was deferred to that phase, and has since been extracted there, so FTP, FTPS and SFTP transfers now run through this engine and this process; findings that belong to those backends are tracked in [`../phase-11-transfer-protocol-clients/PLAN.md`](../phase-11-transfer-protocol-clients/PLAN.md), not here), cookies/authentication/proxies, turning a clicked file link during browsing into a download automatically, and Windows support (the whole workspace currently relies on `UnixStream`, so this stays Unix-only).

---

## 1. Decisions to confirm

**Rows marked ⚠ differ from the current `PLAN.md`, or settle something `PLAN.md` left open.**

| # | Decision | Proposed default | Rationale |
|---|----------|------------------|-----------|
| D1 ⚠ | HTTP client | Keep `ureq` 3; do not switch to `reqwest` | The codebase is synchronous + threads (`session.rs`, `gatekeeper_client`, the MCP server's `spawn_blocking`), so there is no need to pull tokio into the engine path. One thread per segment |
| D2 ⚠ | Probe method | `GET` with `Range: bytes=0-0`, instead of the `HEAD` in `PLAN.md` | Tests what the server actually does rather than what it advertises, and also works with servers that reject `HEAD` |
| D3 | Layering | The transfer engine lives in `backend/net` (a library); `backend/downloads` is the process that wraps it | Reconciles "a `blueice-net` feature" with `PLAN.md`'s isolated process. The engine can be fully test-driven against a local test server |
| D4 ⚠ | Gatekeeper | Two stages: `CheckUrl` → (probe) → a new `CheckDownload{url,file_name,content_type,total_bytes}`. A `DownloadClearance` is issued only if both clear. An unreachable gatekeeper counts as a rejection. Resuming re-runs the review | Matches Phase 7's typestate token and fail-closed policy. `blueice-net` gains a dependency on `blueice-ipc` so the engine itself can require the token |
| D5 | File layout | Data goes into a pre-allocated `<dest>.blueice-part`; the sidecar is `<dest>.blueice-part.json`; on completion, `fsync` then atomic rename. Existing files are not overwritten by default | `dest` never holds a partially written file |
| D6 ⚠ | Resume across browser restarts (an open question in `PLAN.md`) | Yes. Resume only when the server supplied an `ETag` or `Last-Modified`; otherwise restart from scratch, and also restart if validation fails, so old and new content are never spliced. A `transfers.json` index under `$XDG_DATA_HOME/blueice/downloads/` keeps history and resumable items across restarts | `PLAN.md`'s sidecar design already anticipates this; the index is what lets the process be torn down while idle |
| D7 ⚠ | State machine | Beyond Queued/Active/Paused/Completed/Failed, add `AwaitingClearance`, `Blocked`, and `Cancelled`. Each transfer also carries `speed_bps`, `eta_secs`, `segments[]`, `retries`, `last_error`, `blocked{reason,category}`, `mode` (segmented / single-stream, with the reason), `resume_safe`, and `events[]` (the last 32 events) | Lets an AI agent see exactly what is happening, e.g. "server did not send Accept-Ranges; fell back to a single stream". **Extends the state list in `PLAN.md`** |
| D8 | Default parameters (all configurable) | 8 connections per transfer, 1 MiB minimum split size, 5 retries, exponential backoff 0.5 s → 8 s, 30 s stall timeout, 3 concurrent transfers (the rest queue) | These are my proposed defaults, not Free Download Manager's official ones |
| D9 | Servers without Range support | Single stream. Resuming after a pause restarts from byte 0, and the `events` log says so | Report honestly instead of pretending to resume |
| D10 ⚠ | Download-directory confinement | Default to `$BLUEICE_DOWNLOAD_DIR`, otherwise `<XDG download dir or ~/Downloads>/BlueIce`. `dest` accepts only a relative path inside that directory; absolute paths and `..` are rejected. `Content-Disposition` file names are sanitized | Phase 7's risk taxonomy explicitly lists an AI agent reaching the file system outside a downloads directory |
| D11 ⚠ | Private-network URLs (localhost / RFC 1918) | **Not blocked** in this slice; recorded as a known gap for the gatekeeper's rule base | Tests need localhost, and blocking these deserves its own design |
| D12 | Process lifecycle | Register `downloads` in the launcher's `ProcessRegistry` as an `IdleTeardown` slot (mirroring `mcp-server`: typed, but not yet wired to automatic spawn). Clients use connect-or-spawn (if the socket is missing, spawn the sibling `blueice-downloads` binary) | Automatic teardown wiring is the unfinished part of Phase 8; this slice does not widen it |
| D13 | Progress-bar width | Verify first whether layout supports percentage widths; if not, use px (the page generator computes the inner width) | Resolved: layout supports percentage widths; M5 uses them and has a layout test |
| D14 ⚠ | How a human starts a download | `frontend-reference` gains stdin commands `downloads` (opens `about:downloads`) and `download <url>` (talks to the downloads process directly, without extending `ClientMessage`) | Same style as the existing `credits` command; avoids widening `core`'s protocol surface |
| D15 | Process | Milestones M0→M6 in order, one commit per layer. M3/M4/M5 can run in parallel once M1 and M2 are done; sequential by default | Lower coordination risk |

---

## 2. Milestones and work items

Every item is held to the Definition of Done: **write the failing test first (TDD)**, add an end-to-end test wherever a public path exists, do a dedicated test-review pass once the implementation is otherwise complete, and hold new crates to ≥90% line coverage.

### M0 — Design documents (design-first, before any code)
- [x] Add a "Wiring design (resolved 2026-09-20)" section to [`PLAN.md`](PLAN.md): conclusions for the three open questions (D3/D6/D1), plus D2, D4, D7, D10, the D11 known gap, and the `about:downloads` approach
- [x] Update `PLAN.md`'s status and checklist item by item (the `reqwest` evaluation outcome, the file-management surface, the cross-restart decision, and the Phase 11 relationship: one shared subsystem, with the `TransferBackend` trait deferred to Phase 11)
- [x] Decisions in §1 confirmed in review before M1 starts

### M1 — Contract: `blueice-ipc`
- [x] `GatekeeperRequest::CheckDownload { url, file_name, content_type, total_bytes }`: round-trip test; add a matching test to the `ai-gatekeeper` stub
- [x] New module `blueice_ipc::downloads`: `DownloadsRequest` (Hello/Start/List/Get/Pause/Resume/Cancel/Remove/Subscribe/Shutdown), `DownloadsReply` (including the pushed `Updated`), `TransferInfo`, `TransferState`, `SegmentInfo`, `TransferEvent`
- [x] Envelope carries `request_id` (so replies can be matched when pushes interleave); unknown variants fail soft (as `ClientMessage` does); `Hello` + `protocol_version` handshake
- [x] `default_downloads_socket_path()` (same convention as the gatekeeper, separate file name)
- [x] A generic blocking client helper over `Read + Write`, shared by the MCP server, `core`, and `frontend`
- [x] Tests: real `UnixStream` round trips, malformed JSON returns an error instead of panicking, unknown variants

### M2 — Transfer engine: `blueice-net::download`
New dependencies: `serde`, `serde_json`, `blueice-ipc` (all already in the workspace lock file).

Pure functions (first, since they are the easiest to test):
- [x] Segment planning: initial split (`n = min(connections, ceil(total / min_split))`, remainder handling) and dynamic re-split (take the back half of the largest remaining segment)
- [x] Progress / speed / ETA: EMA smoothing, with time injectable (callers pass an `Instant`)
- [x] File names: `Content-Disposition` parsing (including `filename*=`), deriving a name from the URL path, and sanitizing (path separators, control characters, reserved names, excessive length)
- [x] Sidecar: serialization and load-time validation (version, total, validator, data-file length, segments contiguous and non-overlapping, `pos` within range)

Network and files:
- [x] Probe: `Range: 0-0` → `206` + `Content-Range` gives the total; `200` means no Range support; the final URL after redirects (`ResponseExt::get_uri`); ETag / Last-Modified / Content-Type
- [x] `clearance` module: `Reviewer`, `UrlCleared`, `DownloadClearance` (privately constructed, not `Clone`). `probe()` requires `UrlCleared`; `Transfer::begin()` requires `DownloadClearance`. **Tests must prove** that a rejection, an unreachable gatekeeper, and a timeout each fail to produce a token (fail-closed)
- [x] Segment worker: Range requests, positioned writes, `If-Range`, checking for `206` and the `Content-Range` start offset, and failing on an ETag mismatch (never splice)
- [x] Coordinator thread: each tick samples speed, publishes a snapshot, checkpoints when due (`sync_data`, then an atomic sidecar write), tops up workers, and runs the stall watchdog
- [x] Dynamic re-split: a connection that finishes splits the largest remaining segment; the worker being split stops when it reads its new `end`
- [x] Retries: retryable (network errors, 408/429/5xx) vs. fatal (other 4xx, 416, resource changed); reset the consecutive-failure count on progress; exponential backoff
- [x] Pause/resume/cancel: per-segment owner tokens so a stale worker's writes are discarded; cancel removes `.blueice-part*`
- [x] Single-stream fallback (including unknown length); restart entirely on a truncated body; restart from 0 after a pause (D9)
- [x] Resume: restore from the sidecar; each invalidation case (server changed, length mismatch, missing validator, missing data file, corrupt sidecar) restarts from scratch and records an event
- [x] Completion: `fsync`, atomic rename, delete the sidecar; known-length truncation is detected while reading, before pre-allocated data could make a finalize-time length check vacuous; fail clearly if `dest` exists and overwriting is not allowed
- [x] Error types: disk full, permissions, DNS/connection, TLS, and status codes, each with a human-readable message
- [x] Local HTTP server for integration tests (`tests/common`, hand-rolled): with/without Range support, changing ETags, N × 5xx before success, a mid-transfer disconnect, throttling, recording each request's Range and the concurrent-connection count, redirects, and `Content-Disposition`
- [x] End-to-end tests: connections really run in parallel (concurrent connections > 1), dynamic re-splitting really happens (a slow segment gets split), large-file content is byte-for-byte correct, pause → resume, resume after a restart (drop the `Transfer` and rebuild it), and a server change mid-transfer is detected

### M3 — `backend/downloads` process
- [x] New crate `blueice-downloads` (lib + `blueice-downloads` bin), added to the workspace members
- [x] `TransferManager`: id allocation, queueing (3 at a time), and the state machine Queued → AwaitingClearance → Active ⇄ Paused → Completed/Failed/Cancelled/Blocked
- [x] A worker thread per transfer: `review_url` → probe → `review_download` → `Transfer::begin`, checking for pause/cancel between steps
- [x] Persistence: `transfers.json` (atomic writes) loaded at startup; anything that was Active becomes Paused (reason: process restarted)
- [x] Download-directory confinement and `dest` resolution (D10); file-name conflict handling
- [x] Unix-socket server: handshake, request dispatch, `Subscribe` push (a slow client only delays itself)
- [x] Register `downloads` in the launcher's `ProcessRegistry` (D12)
- [x] Tests: manager unit tests (fake gatekeeper, local HTTP server); a **real-subprocess end-to-end test** (like `core_binary.rs`): start the binary, complete a download over the socket, get rejected by the gatekeeper, pause/resume, and still see history after a restart

### M4 — MCP tools (`blueice-mcp-server`)
- [x] Tools: `download_file(url, dest?)`, `list_transfers(state?)`, `get_transfer(id)`, `pause_transfer`, `resume_transfer`, `cancel_transfer`, `remove_transfer`
- [x] Return structured JSON plus a one-sentence plain-language `summary` (e.g. "8 connections, 41%, 12.3 MB/s, about 1 min 20 s left", "blocked by the gatekeeper: <reason>")
- [x] Wrap every result that carries server-supplied text (URLs, file names, error messages, events) with `wrap_untrusted_page_content`
- [x] Connect-or-spawn the downloads process (logic in `lib.rs`; `server.rs` stays thin `rmcp` glue)
- [x] Tests: unit tests against a fake downloads responder; a real-subprocess test; an automated JSON-RPC session over stdio (`initialize` / `tools/list` / `tools/call`) against the compiled `blueice-mcp-server` — a scripted MCP client, not Claude Code itself, which remains Phase 12's open item
- [x] Update the server `instructions` in `get_info()`

### M5 — `about:downloads` visualization
- [x] **First step**: verify whether layout supports percentage widths (D13) and decide between `%` and px
- [x] Add `downloads.ftl` to `blueice-i18n` (`en` + `zh-TW`); every UI string goes through i18n
- [x] `downloads_html(transfers, locale)`: one row per transfer — file name, state, progress bar, downloaded/total, speed, ETA, a segment block map (per-segment completion, in the style of Free Download Manager), and retries / error / blocked reason
- [x] `Page::navigate("about:downloads")` is generated as a built-in page, like `about:credits`; when the downloads process is unreachable, show an empty state rather than failing
- [x] Live updates: on `session.rs`'s poll tick, if a tab is on `about:downloads`, fetch a fresh snapshot within ~500 ms and re-render only when it changed; `FrameReady` and the matching `Representation` keep sharing one `generation`
- [x] `frontend-reference`: stdin commands `downloads` and `download <url>` (D14)
- [x] Tests: unit tests for the HTML generator (every state, the empty state, zh-TW); a session end-to-end test with a fake downloads server (the page content actually changes with progress, and an AI agent's `get_page_representation` reads the same data); no `#paint` fixture (the page is built from live data, so a fixed fixture would only pin the CSS text); instead the page was rendered to PNG and looked at, in both locales, with every state present

### M6 — Wrap-up
- [x] **Test-review pass**: re-read the content of every new test (not just green status and coverage) — fill in edge cases and error paths, delete tests that no longer check anything meaningful
- [x] `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
- [x] Coverage: `cargo llvm-cov ... --fail-under-lines 90`; if the new crate's `main.rs` is pure wiring, decide whether to add it to `--ignore-filename-regex` following existing practice, and keep `.github/workflows/ci.yml` and `TEST_PLAN.md` in sync
- [x] Documentation sync: this phase's `PLAN.md` status and checklist, Phase 12's checklist (`download_file` / `list_transfers`), Phase 8's checklist (registry registration), Phase 7's checklist (clearance for downloads), the phase table in `BROWSER_CORE_PLAN.md`, `CLAUDE.md` project status, and `README.md`
- [x] Land each milestone as its own commit

---

## 3. Post-slice audit follow-ups (2026-09-21)

Source: an audit of the last 20 commits on this branch. Provenance is marked per item: **(reproduced)** means the audit ran it against the compiled binary or test, **(read)** means it comes from reading the code and was not run. Line numbers are at `5932f5d` and will drift. Same rule as every other item here: **write the failing test first**, then fix, and record each fix's conclusion back into [`PLAN.md`](PLAN.md).

### P1 — a user can hit these in normal use

- [x] **A 0-byte download never reaches `Completed`** — fixed: `TransferManager` applies the final snapshot returned by `Transfer::wait()`, including the synchronous empty-file path. `an_empty_download_reaches_completed_and_releases_the_manager_slot` covers the manager/protocol result, final timestamp, file, and idle state.
- [x] **A stalled connection permanently consumes a worker slot, so a transfer can hang forever** — fixed: capacity is now measured by current segment owners; the watchdog revokes the owner before the coordinator creates replacement claims, so a physical thread stuck in `read()` cannot reserve logical transfer capacity. `every_stalled_connection_is_replaced_without_waiting_for_its_read_to_return` stalls all four initial connections and completes before their eight-second body hold ends. A second guard handles a peer that repeatedly sends a little data (resetting ordinary retry counts) and then stalls: each transfer has a 32-worker abandoned-read ceiling by default and fails explicitly at that ceiling. `repeatedly_stalled_reads_are_bounded_instead_of_accumulating_threads_forever` sets it to two and proves the third abandoned read fails instead of creating a fourth.
- [x] **Destination collisions give silently wrong content** — fixed: active claims compare case-insensitively, resume rechecks competing claims while holding the manager lock, and requested internal transaction-file suffixes are refused. Protocol tests cover case-only destinations, a failed transfer resuming into a newly claimed destination, and every reserved suffix.
- [x] **The default socket path is unusable on macOS** — fixed: `blueice-ipc::local_socket` uses `libc::getuid()` in the one helper shared by gatekeeper/downloads (and the sibling protocols); the directory is owner-verified `0700` and bindings use `umask(077)` plus `0600`. Unit tests prove the UID derivation and initial directory/socket modes.

### P2 — security hardening and integrity

- [x] **`transfers.json` and the data directory are world-readable** — fixed: owner-verifying private-directory creation makes the data and download roots `0700`; `transfers.json`, sidecars and part files are created (and reused part files corrected) as `0600`. HTTP(S) URLs are parsed and userinfo is refused before persistence. Store, sidecar and manager/protocol tests cover the modes and rejection.
- [x] **Local IPC availability holes** — fixed: all IPC frames are capped at 8 MiB before allocation; downloads has a 64-connection permit budget and write deadlines; and the gatekeeper accepts checks concurrently through deadline-bound streams. Downloads requires a complete first `Hello` frame within five seconds using a bounded reader that is cancelled by socket shutdown and joined before its permit is released, so a silent or partial peer cannot leak a thread, file descriptor, or connection slot. Tests cover oversized length rejection, permit exhaustion/release, and a partial `Hello` deadline.
- [x] **`DownloadClearance` was not fully bound to the fetched URL** — fixed: it binds and `Transfer::begin` checks both requested and probe-final URLs, as well as name, type and size. `Probe` fields are crate-private and it is no longer `Clone`, so an external caller cannot forge an altered reviewed response; compile-fail doctests cover non-constructible `UrlCleared` and non-cloneable `Probe`/`DownloadClearance`. **Documented boundary:** ureq can still follow a later redirect from the recorded final URL for each range request (up to its default limit); those hops are not separately reviewed.
- [x] **No size or free-space bound** — fixed: `DownloadOptions` has configurable `max_total_bytes` (100 GiB by default) and `min_free_space_bytes` (1 GiB by default). `Transfer::begin` checks `statvfs` before creating a part file; unknown-length streams enforce the maximum before every write. Tests cover an oversized known resource before file creation, an unknown-length stream exceeding the limit, and the pure free-space arithmetic boundary.
- [x] **Tool text overstates the protection** — fixed: `download_file` and server `instructions` now call it a local gatekeeper hook and explicitly state that its rule base is always-clear and private/link-local URLs are not blocked; they say it is not malware scanning, authorization, or SSRF protection.
- [x] **File names kept bidi and invisible characters** — fixed: `sanitize` removes bidi overrides, zero-width/invisible formatting characters and Unicode tags; preserves a safe filename suffix when truncating; and obtains fallback names from parsed URL paths rather than queries. Unit tests cover each class. The `about:downloads` fixture receives the sanitized `invoicefdp.exe` result, and a real MCP/downloads test sends the RFC 8187 `filename*=UTF-8''invoice%E2%80%AEfdp.exe` header: the completed and list records contain only the safe name, never U+202E.
- [x] **Unbounded growth** — fixed: transfer plans, dynamic re-splitting, resumed sidecars and snapshots cap segments at 1,024; history retains at most 1,000 records (evicting only terminal records) and the queue accepts at most 100 waiting transfers by default. URLs and persisted remote metadata have a 4 KiB limit; oversized metadata is rejected and diagnostic, event, gatekeeper, snapshot and old-store text is UTF-8-safely truncated with a marker. Sidecar input is size-limited, and tests cover queue/history eviction, oversized gatekeeper/store text, URL rejection, resumed segment limits, and UTF-8-safe truncation.
- [x] **Destination confinement gaps** — fixed: requested destinations validate `dest`, part and sidecar paths (including dangling links); derived names treat dangling transaction links as taken; and all transaction reads/writes walk parent directories through `openat(O_NOFOLLOW)` before operating relative to the retained fd. New files use `create_new`, default completion uses no-replace hard-link/unlink, and renames sync the parent directory. Tests cover final and parent symlinks, transaction links, and sidecar reads.

### P3 — correctness and robustness

- [x] **`remove` did not notify subscribers** — fixed: removal now publishes a distinct, bounded `Removed { id }` change through the manager and downloads IPC, so a subscriber can delete its stale row. Removing a `Blocked` transfer now clears its pre-existing part file and sidecar as it already did for `Failed`; protocol tests cover both behaviors.
- [x] **Store details** — fixed: corrupt stores receive unique `.corrupt`, `.corrupt.1`, … names; `u64::MAX` saturates/exhausts instead of wrapping; and `TransferManager::open` re-confines every loaded destination, scrubbing and failing an unsafe one. A single persistence worker now coalesces only superseded snapshots and performs atomic write/`fsync` outside the state mutex. It logs and retains the last save error until a later success (available through `last_persistence_error`), while startup, orderly shutdown, and `Remove` wait for their snapshot so they cannot falsely report durable success. Tests cover the original cases, retained failures, latest-snapshot coalescing, and the removal persistence boundary.
- [x] **`finalize_file`'s length check was vacuous** — fixed: removed the misleading check after confirming known-length truncation is rejected by `at_end_of_stream` before finalization. Pre-allocated segmented files cannot make a short body look complete.
- [x] **`Last-Modified` was used without RFC 9110's strength check** — fixed: HTTP retains it as a validator only when a parseable response `Date` is at least 60 seconds later (the RFC's required safety margin); otherwise only a strong ETag can make the transfer resumable. Sidecar version 2 refuses pre-fix sidecars, and probe tests cover a 59-second response.
- [x] **`ureq` could transparently gunzip an ignored `identity` request** — fixed: `ureq` is now built without its default `gzip` feature, because that feature decodes solely from the response `Content-Encoding` header and removes that header before range validation. The HTTP boundary additionally rejects every non-`identity` `Content-Encoding` response before metadata or body bytes are used. Probe tests cover a server that ignores `Accept-Encoding: identity` and replies `gzip`.
- [x] **Launcher idle teardown was unsafe to wire** — fixed conservatively: `downloads` is no longer registered in the generic time-only `ProcessRegistry`. Its clients retain connect-or-spawn, but the launcher cannot accidentally own and `SIGKILL` it until a future integration can query zero active/queued transfers and request an orderly shutdown. The supervisor test proves the default fleet rejects a downloads child rather than scheduling it for teardown.

### `about:downloads`, the MCP adapter and the frontend (M4/M5 owned)

- [x] **The refresh push broke the "frame and `Representation` share one `generation`" invariant** — fixed: `Page` owns a monotonic frame generation per tab, and `GetRepresentation` reports that tab's latest value. Frame files are named and retained by `(tab_id, generation)`, so a busy downloads tab prunes only its own old frames. MCP now accepts only replies carrying its active request id; an unsolicited human-tab refresh cannot complete a navigation or change an unqualified screenshot target. The two-tab test keeps an active `about:downloads` tab rendering beyond the old retention window and proves the other tab's frame remains readable and its representation still has its own generation; IPC and MCP tests cover per-tab retention and broadcast isolation. Refresh creates a new document, so old Node IDs intentionally remain safe no-ops as documented in `PLAN.md`.
- [x] **The `rendered` cache went stale after a same-URL navigation** — fixed: every navigation to `about:downloads` starts a new visit token, clears its rendered/cache scheduling state, and rejects an older in-flight result. The next refresh therefore repaints the new document even when the list HTML is otherwise unchanged; a focused test covers the same-URL case.
- [x] **Transient stalls flipped the page to "not running"** — fixed: `fetch_within` cancels and joins its complete exchange at the deadline, so a partial peer cannot leak a thread or descriptor; and the refresher retains an already-successful render through one failed poll. Only two consecutive failures replace useful data with the unavailable view, while a success resets the failure count. The unit test proves the first/second failure boundary.
- [x] **The MCP downloads client had no timeouts** — fixed: every `DownloadsHandle` socket has a 20 s read/write deadline (headroom over the manager's 15 s pause settle), and a call takes the cached connection out of the mutex before I/O. A hung exchange is discarded at its deadline while another tool opens its own connection. IPC now distinguishes request-write from reply-read failures; before a non-idempotent request, a reused cached socket receives an idempotent liveness read, so a dead post-restart socket is reconnected before the mutation is serialized. Tests prove a hung call does not block a second tool call and a stale cached connection transparently reaches a fresh process for `Start`; a lost reply after a mutation remains deliberately non-retried.
- [x] **`frontend-reference` `download <url>` mishandled stale sockets and stdio** — fixed: after spawning it polls `UnixStream::connect` itself, not pathname existence, so a leftover socket entry cannot produce a premature failure. The detached downloads child now has stdin/stdout/stderr set to `null`, matching the sibling spawners. A test creates a stale entry, replaces it later with a live fake service, and proves the command connects successfully.

### Test debt (found by re-reading the suite, not by coverage)

- [x] Missing P1 cases — covered: the manager-level empty-file completion, case-insensitive destination collision, a failed transfer whose destination was claimed before resume, and bidi filename output are in `downloads/tests/protocol.rs` / `mcp_downloads.rs`; `net/tests/transfer.rs` proves four independently stalled workers are replaced and refuses a dangling destination symlink without creating its target.
- [x] `every_transfer_result_is_framed_as_untrusted_data` — strengthened: it now checks successful records, list output, transfer errors, rejected starts, a hostile filename containing the framing marker, and a hostile gatekeeper `blocked.reason`; `transfer_result` and failed lists apply the same untrusted-data wrapper to errors.
- [x] Stale workarounds — removed and proved end-to-end: MCP navigation now returns its loaded snapshot, links use a relative href, and a linked `click` first detects the link then waits for `Navigated` plus `FrameReady` before asking for a snapshot. Unit and real MCP-wire tests cover that barrier.
- [x] Timing and hygiene: `about:downloads` no longer uses fixed settle loops or a sleep to prove polling stopped; the hung-service test now waits for the fake service to record that the background poll reached its stalled peer, then proves a correlated resize frame still arrives, without a wall-clock speed assertion. `TransferManager::is_idle()` now also requires no job thread still unwinding, so pause/restart/shutdown tests use the real lifecycle barrier rather than sleep-based "never auto-started" guesses. `a_subscriber_that_never_reads_cannot_stall_other_clients` constrains and reads the actual Unix receive window (`SO_RCVBUF`/`FIONREAD`), fills it with distinct near-limit queued updates, then proves another client can still `Get`; it no longer infers socket pressure from a broad elapsed time. The core-binary downloads test has a cleanup guard for both children, and the built-in-page test makes an actual link click.
- [x] The `ai_snapshot.rs` DOM-ID assertion now finds the actual `<p>` node and compares its precise stable ID to the represented node, rather than merely checking that a small integer resolves somewhere in the document.
- [x] The M6 test-review follow-up was done by reading assertions rather than relying on coverage: it found and fixed untrusted transfer-error framing, the stale MCP link-click snapshot, the false link test, and the weak DOM-ID assertion. The historical 96.7% total-coverage figure remains intentionally unclaimed because `cargo llvm-cov` still needs the external `llvm-tools-preview` component.

### Cross-phase (owned elsewhere; listed because they block this phase's Definition of Done)

- [x] **`cargo test --workspace` did not terminate at HEAD.** Fixed: `navigate_addresses_the_given_tab_id_on_the_wire`'s fake core now sends the protocol-required `FrameReady` after `Navigated`, matching `CoreConnection::navigate`'s intentional wait before it asks for the representation. `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` now finish cleanly. The separate lack of a deadline for `CoreConnection::navigate` and `blueice_net::fetch` remains Phase 12 work ([`../phase-12-mcp-server/PLAN.md`](../phase-12-mcp-server/PLAN.md)).
- [x] **Phase 11 backend findings** — fixed in [`../phase-11-transfer-protocol-clients/PLAN.md`](../phase-11-transfer-protocol-clients/PLAN.md): FTP propagates its final control reply and pins passive data connections to the control peer; SFTP's segmented shape is documented and every non-match known-host result fails closed; FTPS uses `suppaftp` 12.0.1's default native-TLS verifier with the endpoint hostname; credential secrets are redacted from IPC `Debug` and accepted only from the local stdin command, never an MCP argument.
- [x] **Documentation sync for this phase** — fixed: `PLAN.md` now states the owner-token watchdog's liveness boundary, the clearance's requested-and-final URL binding, and later unreviewed range redirects; documents the per-tab frame contract, MCP deadlines, and the intentional absence of a time-only downloads teardown slot; and describes the current 10 non-secret download-related MCP tools plus the local stdin credential command. `CLAUDE.md` has the same current tool count and no longer calls the completed Phase 11 protocol backends unbuilt.

---

## 4. Known risks and unverified items

- **`ureq` has no per-`read` timeout.** Every `timeout_*` setting I found is a whole-phase timeout, which does not suit long downloads. Stall detection therefore lives in the coordinator's watchdog: it revokes the stalled segment's owner and reassigns it. A thread blocked in a `read()` can, in the worst case, survive until the TCP layer reports an error; its stale owner makes its writes harmless and it cannot consume a logical connection slot. A peer that repeatedly sends a little progress and stalls could otherwise accumulate such physical reads while resetting the ordinary retry counter, so `DownloadOptions::max_abandoned_workers` caps them at 32 per transfer by default; reaching the cap fails the transfer explicitly rather than creating another thread/socket.
- **Sparse files.** Pre-allocating with `set_len` does not fail on a full disk until bytes are written; this is covered under error handling.
- **The hand-rolled HTTP test server** determines how far every engine test can be trusted, so it gets its own basic tests first.
- **Wire-level enforcement.** A token does not cross a process boundary. This slice has the downloads process query the gatekeeper itself and not trust its callers (the MCP server included). IPC-level enforcement remains the existing open item from Phase 9 and is not solved here.
- **`ai-gatekeeper` is currently an always-clear stub**, so the "blocked" path can only be tested with a fake gatekeeper; it does not exercise real review content.

## 5. Requests for the reviewer

1. Should any of D1–D15 change? The ⚠ rows (D1, D2, D4, D6, D7, D10, D11, D14) most need a look.
2. Should the scope or the order of the milestones change?

This file lives in `development/browser_core/phase-10-download-manager/`, next to this phase's [`PLAN.md`](PLAN.md). `PLAN.md`'s own checklist is phase-level; this file is the working list for the first slice, and M0 and M6 write the conclusions back into `PLAN.md`.
