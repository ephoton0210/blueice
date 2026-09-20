# TODO — BlueIce download manager (Phase 10, first slice)

[← Back to Phase 10 plan](PLAN.md)

> **Status: decisions confirmed in review; design recorded in [`PLAN.md`](PLAN.md)'s "Wiring design (resolved 2026-09-20)", which is the authoritative record if the two ever differ. M0 (design) and M1 (`blueice-ipc` contract) are done; M2 onward has not started.**
> Branch: `feature/downloads-first-slice`.
> Design inputs: [`PLAN.md`](PLAN.md) (this phase), [`../phase-7-local-ai/PLAN.md`](../phase-7-local-ai/PLAN.md), [`../phase-12-mcp-server/PLAN.md`](../phase-12-mcp-server/PLAN.md), [`../research/safe-browsing-enforcement.md`](../research/safe-browsing-enforcement.md), [`../research/multi-process-memory.md`](../research/multi-process-memory.md).

## 0. Goal and scope

**Goal**: match the core capabilities of Free Download Manager, and make every transfer fully observable to an AI agent over MCP.

1. **Segmented, resumable, accelerated downloads**: multi-connection segmented transfer, dynamic re-splitting, pause/resume, retries, single-stream fallback.
2. **Visualization**: a built-in `about:downloads` page rendered by BlueIce's own engine (decided), so a human and an AI agent see the same render pass.
3. **AI observability**: MCP tools report progress, speed, ETA, per-segment state, retries, errors, and what the safety gatekeeper decided.

**Scope**: the engine, the `downloads` process, the MCP tools, and the visualization, delivered together (decided).

**Explicitly out of scope for this slice**: a scheduler, speed limits, multi-mirror/multi-source downloads, FTP/SFTP (Phase 11; the shared `TransferBackend` trait is deferred to that phase, and the HTTP range fetch stays behind one internal function boundary so extracting it later is mechanical), cookies/authentication/proxies, turning a clicked file link during browsing into a download automatically, and Windows support (the whole workspace currently relies on `UnixStream`, so this stays Unix-only).

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
| D13 | Progress-bar width | Verify first whether layout supports percentage widths; if not, use px (the page generator computes the inner width) | Only the CSS value layer is confirmed to have `Percentage`; layout is not yet verified |
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
- [ ] Segment planning: initial split (`n = min(connections, ceil(total / min_split))`, remainder handling) and dynamic re-split (take the back half of the largest remaining segment)
- [ ] Progress / speed / ETA: EMA smoothing, with time injectable (callers pass an `Instant`)
- [ ] File names: `Content-Disposition` parsing (including `filename*=`), deriving a name from the URL path, and sanitizing (path separators, control characters, reserved names, excessive length)
- [ ] Sidecar: serialization and load-time validation (version, total, validator, data-file length, segments contiguous and non-overlapping, `pos` within range)

Network and files:
- [ ] Probe: `Range: 0-0` → `206` + `Content-Range` gives the total; `200` means no Range support; the final URL after redirects (`ResponseExt::get_uri`); ETag / Last-Modified / Content-Type
- [ ] `clearance` module: `Reviewer`, `UrlCleared`, `DownloadClearance` (privately constructed, not `Clone`). `probe()` requires `UrlCleared`; `Transfer::begin()` requires `DownloadClearance`. **Tests must prove** that a rejection, an unreachable gatekeeper, and a timeout each fail to produce a token (fail-closed)
- [ ] Segment worker: Range requests, positioned writes, `If-Range`, checking for `206` and the `Content-Range` start offset, and failing on an ETag mismatch (never splice)
- [ ] Coordinator thread: each tick samples speed, publishes a snapshot, checkpoints when due (`sync_data`, then an atomic sidecar write), tops up workers, and runs the stall watchdog
- [ ] Dynamic re-split: a connection that finishes splits the largest remaining segment; the worker being split stops when it reads its new `end`
- [ ] Retries: retryable (network errors, 408/429/5xx) vs. fatal (other 4xx, 416, resource changed); reset the consecutive-failure count on progress; exponential backoff
- [ ] Pause/resume/cancel: per-segment owner tokens so a stale worker's writes are discarded; cancel removes `.blueice-part*`
- [ ] Single-stream fallback (including unknown length); restart entirely on a truncated body; restart from 0 after a pause (D9)
- [ ] Resume: restore from the sidecar; each invalidation case (server changed, length mismatch, missing validator, missing data file, corrupt sidecar) restarts from scratch and records an event
- [ ] Completion: `fsync`, length check, rename, delete the sidecar; fail clearly if `dest` exists and overwriting is not allowed
- [ ] Error types: disk full, permissions, DNS/connection, TLS, and status codes, each with a human-readable message
- [ ] Local HTTP server for integration tests (`tests/common`, hand-rolled): with/without Range support, changing ETags, N × 5xx before success, a mid-transfer disconnect, throttling, recording each request's Range and the concurrent-connection count, redirects, and `Content-Disposition`
- [ ] End-to-end tests: connections really run in parallel (concurrent connections > 1), dynamic re-splitting really happens (a slow segment gets split), large-file content is byte-for-byte correct, pause → resume, resume after a restart (drop the `Transfer` and rebuild it), and a server change mid-transfer is detected

### M3 — `backend/downloads` process
- [ ] New crate `blueice-downloads` (lib + `blueice-downloads` bin), added to the workspace members
- [ ] `TransferManager`: id allocation, queueing (3 at a time), and the state machine Queued → AwaitingClearance → Active ⇄ Paused → Completed/Failed/Cancelled/Blocked
- [ ] A worker thread per transfer: `review_url` → probe → `review_download` → `Transfer::begin`, checking for pause/cancel between steps
- [ ] Persistence: `transfers.json` (atomic writes) loaded at startup; anything that was Active becomes Paused (reason: process restarted)
- [ ] Download-directory confinement and `dest` resolution (D10); file-name conflict handling
- [ ] Unix-socket server: handshake, request dispatch, `Subscribe` push (a slow client only delays itself)
- [ ] Register `downloads` in the launcher's `ProcessRegistry` (D12)
- [ ] Tests: manager unit tests (fake gatekeeper, local HTTP server); a **real-subprocess end-to-end test** (like `core_binary.rs`): start the binary, complete a download over the socket, get rejected by the gatekeeper, pause/resume, and still see history after a restart

### M4 — MCP tools (`blueice-mcp-server`)
- [ ] Tools: `download_file(url, dest?)`, `list_transfers(state?)`, `get_transfer(id)`, `pause_transfer`, `resume_transfer`, `cancel_transfer`, `remove_transfer`
- [ ] Return structured JSON plus a one-sentence plain-language `summary` (e.g. "8 connections, 41%, 12.3 MB/s, about 1 min 20 s left", "blocked by the gatekeeper: <reason>")
- [ ] Wrap every result that carries server-supplied text (URLs, file names, error messages, events) with `wrap_untrusted_page_content`
- [ ] Connect-or-spawn the downloads process (logic in `lib.rs`; `server.rs` stays thin `rmcp` glue)
- [ ] Tests: unit tests against a fake downloads responder; a real-subprocess test; one manual `initialize` / `tools/list` / `tools/call` session with a real MCP client (the same way the existing tools were verified)
- [ ] Update the server `instructions` in `get_info()`

### M5 — `about:downloads` visualization
- [ ] **First step**: verify whether layout supports percentage widths (D13) and decide between `%` and px
- [ ] Add `downloads.ftl` to `blueice-i18n` (`en` + `zh-TW`); every UI string goes through i18n
- [ ] `downloads_html(transfers, locale)`: one row per transfer — file name, state, progress bar, downloaded/total, speed, ETA, a segment block map (per-segment completion, in the style of Free Download Manager), and retries / error / blocked reason
- [ ] `Page::navigate("about:downloads")` is generated as a built-in page, like `about:credits`; when the downloads process is unreachable, show an empty state rather than failing
- [ ] Live updates: on `session.rs`'s poll tick, if a tab is on `about:downloads`, fetch a fresh snapshot within ~500 ms and re-render only when it changed; `FrameReady` and the matching `Representation` keep sharing one `generation`
- [ ] `frontend-reference`: stdin commands `downloads` and `download <url>` (D14)
- [ ] Tests: unit tests for the HTML generator (every state, the empty state, zh-TW); a session end-to-end test with a fake downloads server (the page content actually changes with progress, and an AI agent's `get_page_representation` reads the same data); a `#paint` fixture if feasible

### M6 — Wrap-up
- [ ] **Test-review pass**: re-read the content of every new test (not just green status and coverage) — fill in edge cases and error paths, delete tests that no longer check anything meaningful
- [ ] `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Coverage: `cargo llvm-cov ... --fail-under-lines 90`; if the new crate's `main.rs` is pure wiring, decide whether to add it to `--ignore-filename-regex` following existing practice, and keep `.github/workflows/ci.yml` and `TEST_PLAN.md` in sync
- [ ] Documentation sync: this phase's `PLAN.md` status and checklist, Phase 12's checklist (`download_file` / `list_transfers`), Phase 8's checklist (registry registration), Phase 7's checklist (clearance for downloads), the phase table in `BROWSER_CORE_PLAN.md`, `CLAUDE.md` project status, and `README.md`
- [ ] Land each milestone as its own commit

---

## 3. Known risks and unverified items

- **`ureq` has no per-`read` timeout.** Every `timeout_*` setting I found is a whole-phase timeout, which does not suit long downloads. Stall detection therefore lives in the coordinator's watchdog: it revokes the stalled segment's owner and reassigns it. A thread blocked in a `read()` can, in the worst case, survive until the TCP layer reports an error; it cannot affect data correctness.
- **`ureq` body-streaming details are unconfirmed.** Whether `into_reader` has no size limit is not yet verified; my first source lookup used the wrong path. Verifying this is the first step of M2.
- **Percentage widths in layout** are unverified (D13).
- **Sparse files.** Pre-allocating with `set_len` does not fail on a full disk until bytes are written; this is covered under error handling.
- **The hand-rolled HTTP test server** determines how far every engine test can be trusted, so it gets its own basic tests first.
- **Wire-level enforcement.** A token does not cross a process boundary. This slice has the downloads process query the gatekeeper itself and not trust its callers (the MCP server included). IPC-level enforcement remains the existing open item from Phase 9 and is not solved here.
- **`ai-gatekeeper` is currently an always-clear stub**, so the "blocked" path can only be tested with a fake gatekeeper; it does not exercise real review content.

## 4. Requests for the reviewer

1. Should any of D1–D15 change? The ⚠ rows (D1, D2, D4, D6, D7, D10, D11, D14) most need a look.
2. Should the scope or the order of the milestones change?

This file lives in `development/browser_core/phase-10-download-manager/`, next to this phase's [`PLAN.md`](PLAN.md). `PLAN.md`'s own checklist is phase-level; this file is the working list for the first slice, and M0 and M6 write the conclusions back into `PLAN.md`.
