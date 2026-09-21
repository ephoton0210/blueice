# TODO — BlueIce download manager (Phase 10, first slice)

[← Back to Phase 10 plan](PLAN.md)

> **Status: decisions confirmed in review; design recorded in [`PLAN.md`](PLAN.md)'s "Wiring design (resolved 2026-09-20)", which is the authoritative record if the two ever differ. M0 (design), M1 (`blueice-ipc` contract), M2 (`blueice-net` transfer engine), M3 (the `downloads` process), M4 (the MCP tools), M5 (`about:downloads`) and M6 (wrap-up) are done: the first slice is feature-complete. A post-slice audit (2026-09-21, [§3](#3-post-slice-audit-follow-ups-2026-09-21)) found defects the milestone tests did not catch, including four P1 items (a 0-byte download that never completes, a transfer that hangs after every connection stalls, destination collisions, an unusable default socket path on macOS); those are open work, not part of "done".**
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
- [x] Completion: `fsync`, length check, rename, delete the sidecar; fail clearly if `dest` exists and overwriting is not allowed
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

- [ ] **A 0-byte download never reaches `Completed`** (reproduced). For an empty probe, `Transfer::begin` returns `Finished(Completed)` with no coordinator thread (`transfer.rs:249-255`), so `on_update` never fires. `manager.rs:814` discards the snapshot `transfer.wait()` returns and returns `JobEnd::Engine`, and `run_job` leaves the state alone on `Engine`. The file is created on disk, but the record stays `awaiting_clearance` forever (`finished_at_ms` and `total_bytes` null); `pause`, `resume` and `remove` all refuse, only `cancel` gets out (and marks a finished file `Cancelled`), and `is_idle()` never becomes true. *Test first*: a manager-level test against a server answering `200` with `Content-Length: 0`; `backend/net/tests/transfer.rs:227` covers the empty file at engine level only. *Fix*: use the snapshot `wait()` returns, or have `begin` publish the final snapshot.
- [ ] **A stalled connection permanently consumes a worker slot, so a transfer can hang forever** (reproduced). `Shared::top_up` gates on `workers < max_connections` (`transfer.rs:513-519`) and `workers` drops only when a worker thread exits (`:827`). The watchdog revokes the segment but cannot free a thread stuck in `reader.read()` (the HTTP agent has no body timeout, `http.rs:21-23`). After `max_connections` cumulative stalls no worker can claim the revoked segments; repro: 8 MiB file, 8 segments, each connection sends 100 KB then goes silent, and the transfer stays `active` at 819200 bytes with no new request for 110 s. Each stuck thread also keeps an fd and an `Arc<Shared>`, and after a cancel the unlinked `.part` keeps its disk space. *Test first*: stall **every** connection (the existing test at `transfer.rs:375-399` stalls one of four, so spare slots hide the bug). *Fix*: do not count revoked workers toward the cap, and/or give the reader a real read timeout.
- [ ] **Destination collisions give silently wrong content** (reproduced on macOS). (a) The claim check compares exact-case paths (`manager.rs:293`, `:766-767`), and `policy.rs:98` (`is_taken`) sees nothing until a `.part` exists: `Start dest="Report.bin"` (URL A) then `dest="report.bin"` (URL B) share one `.blueice-part`, transfer 1 renames transfer 2's bytes into `Report.bin` and reports `completed`, and transfer 2 then fails with "already exists". (b) `claims_destination` excludes `Failed`/`Blocked` (`manager.rs:231`) and `resume` (`:389-412`) never re-checks claims: start X, let it fail, start another transfer to X, resume the first, and two Active transfers share a part file, so cancelling or removing the failed one deletes the running one's part and sidecar. (c) A requested `dest` skips any existing `.blueice-part` check, and names ending `.blueice-part`, `.blueice-part.json` or `.tmp` are allowed (`file_name.rs`), so one transfer's destination can be another's live part file or sidecar. *Test first*: two destinations differing only in case; resuming a failed transfer whose destination is now claimed; a `dest` ending in `.blueice-part`. *Fix*: compare claims case-folded (or by file identity), reject the reserved suffixes, and re-check claims on resume.
- [ ] **The default socket path is unusable on macOS** (reproduced). `libc_getuid` (`ipc/src/downloads.rs:475-490`, copied from `gatekeeper.rs`) reads `/proc/self/status` and falls back to `std::process::id()`, so without `XDG_RUNTIME_DIR` the directory is `blueice-<PID of the calling process>`; different processes look in different directories, and the downloads and gatekeeper default sockets never rendezvous. The directory is created `0755` (`cli.rs:80`) and the socket is `chmod 0600` only after `bind` (`cli.rs:88-91`), leaving a window (the test asserts 0600 but not the window). On Linux with `XDG_RUNTIME_DIR` unset, `$TMPDIR/blueice-<uid>` is predictable in a shared `/tmp`, so another local user could pre-create it and stand up a fake `ai-gatekeeper.sock` that clears everything. *Fix*: a real `getuid` in one shared helper (both `gatekeeper.rs` and `downloads.rs` use it), create the directory `0700` and verify its owner, bind under a restrictive `umask`.

### P2 — security hardening and integrity

- [ ] **`transfers.json` and the data directory are world-readable** (reproduced: `0644` / `0755`, `store.rs:80`, `:85`). The file holds URLs (possibly pre-signed or tokenized), destination paths and the gatekeeper's decision text; sidecars hold `final_url` and the ETag. Also, `validate_url` returns `Ok(())` for http(s) without parsing (`backend.rs:96-97`), so `https://user:pass@host/f` is persisted verbatim and shown over MCP and on `about:downloads`. *Fix*: `0600`/`0700`, and refuse (or strip) userinfo for http(s) as SFTP/FTP already do.
- [ ] **Local IPC availability holes** (read). `read_frame_bytes` does `vec![0u8; len]` for a peer-supplied `u32`, up to 4 GiB, with no cap (`ipc/src/lib.rs:219-224`, pre-existing, but `server.rs:29-33` now exposes it with a thread per connection, no connection cap, no `Hello` timeout and no write timeout). The `ai-gatekeeper` binary serves connections sequentially with no read timeout, so one idle local connection blocks every review, and fail-closed then blocks all downloads and navigation; its lib doc says each connection is served by its own accepted connection, which is false. *Fix*: cap frame length, add timeouts and a connection cap, and either serve gatekeeper connections concurrently or correct the doc.
- [ ] **`DownloadClearance` is not bound to the URL actually fetched** (read). `clearance.rs:142` stores `probe.url` while the gatekeeper reviewed `probe.final_url` (`:137`); `Transfer::begin` checks URL, file name, type and size (`transfer.rs:219-231`) but fetches from `probe.final_url` (`:311`), and `Probe` has all-`pub` fields and is `Clone` (`probe.rs:19-37`). Only in-process callers can exploit it, but the token's contract is weaker than `PLAN.md:102` reads. Also undocumented: every later ranged segment request follows up to 10 redirects (`http.rs:22`), and those hops are never reviewed (`PLAN.md:148` documents only the probe). *Test first*: `compile_fail` doctests for `UrlCleared: Clone` and for private construction (only `DownloadClearance: Clone` is covered, `clearance.rs:24-27`). *Fix*: bind and check `final_url`; document the redirect gap.
- [ ] **No size or free-space bound** (reproduced). `transfer.rs:272` and `:281` call `set_len(server-claimed total)`: a server claiming 4 TiB produced a 4 TiB sparse `.part` and an `active` transfer; an unknown-length stream (`:283`) has no cap at all. This relies entirely on the gatekeeper, which is an always-clear stub. *Fix*: a configurable maximum size and a free-space check before `begin`.
- [ ] **Tool text overstates the protection** (read). The `download_file` description and the server `instructions` (`mcp-server/src/server.rs:365-367`, `~491-499`) say every download is "reviewed by the safety gatekeeper", but the gatekeeper is an always-clear stub (§4) and private-network URLs are not blocked (D11), so a prompt-injected agent can fetch from localhost or link-local addresses. *Fix*: say so in the description and `instructions` until the rule base exists.
- [ ] **File names keep bidi and invisible characters** (read). `sanitize` strips only `char::is_control` (`file_name.rs:163-165`), so U+202E, zero-width and Unicode-tag characters survive: `invoice\u{202E}fdp.exe` displays as `invoiceexe.pdf` on `about:downloads` and reaches the LLM through MCP JSON. Also: truncating a long name with no extension can leave a trailing space or dot, and `file_name_from_url` takes the last `/` segment out of the query when there is no path (`https://h?x=/a/b` gives `b`). *Test first*: each of these, with a hostile-name fixture on the page and in the MCP result.
- [ ] **Unbounded growth** (read). `segments` gets a new entry per split and never loses one (`transfer.rs:550-552`), and every snapshot (each 100 ms tick), `TransferInfo`, wire push and `transfers.json` write copies all of them (on the order of total / 1 MiB). Transfer history and the queue have no cap (`start` has none), and remote strings are unbounded: `backend.rs:126` echoes a raw `Content-Range` header (up to about 64 KB per event, 32 events per transfer), inflating `list_transfers` and the page. *Fix*: cap segment count, history and per-string length.
- [ ] **Destination confinement gaps** (read; each needs a local writer inside the download directory). `resolve_requested` (`policy.rs:53-92`) is check-then-use and checks only `dest`, not `dest.blueice-part` or the sidecar; the empty-file branch (`transfer.rs:251`) uses `File::create(dest)`, which follows a dangling symlink; the derived-name path (`manager.rs:763-767`) never goes through `resolve_requested`; `finalize_file` does `exists()` then `rename` (`transfer.rs:811-814`), so a file created in between is silently replaced when `overwrite=false`; and there is no directory `fsync` after the rename. *Fix*: `create_new` / `O_NOFOLLOW`, a no-replace rename (hard link then unlink), and a directory `fsync`. *Test first*: "dangling ones included" is claimed in `d6addd9`'s message but only symlinks to existing targets are tested.

### P3 — correctness and robustness

- [ ] **`remove` does not notify subscribers** (read): `manager.rs:465-487` never calls `commit` and the protocol has no "removed" push, so subscribers keep stale records. `remove` on a `Blocked` transfer also leaves its partial files (`:483` cleans only `Failed`).
- [ ] **Store details** (read): a second corrupt or newer `transfers.json` overwrites the first `.corrupt` (`store.rs:71`), contradicting "never deleted"; `id + 1` can overflow on a crafted file (`:65`); loaded `dest_path`s are not re-confined to the download directory; `persist` rewrites the whole file with `fsync` under the state mutex on every state change and swallows errors (`manager.rs:565-570`), so history loss is silent.
- [ ] **`finalize_file`'s length check is vacuous** (read): `transfer.rs:806-809` compares against a file already `set_len(total)` (`:272`, `:281`), so it can never detect a short write. Truncation is actually caught by `at_end_of_stream` (`:664-667`). Make the check real or delete it.
- [ ] **`Last-Modified` is used as a validator without the RFC 9110 strength check** (at least one second older than `Date`): a small splice risk on frequently modified files (`probe.rs`, `validator()`).
- [ ] **To investigate (unverified)**: `ureq`'s default features may transparently gunzip a `Content-Encoding: gzip` reply even though `identity` was requested, so offsets would disagree with compressed lengths and the transfer would end `Truncated` or `Failed`. Check the features in `Cargo.toml` and add a test with a gzip-encoding server.
- [ ] **Launcher idle teardown is unsafe to wire** (read): `default_fleet` registers `downloads` as `IdleTeardown` (300 s, time only) and `teardown` `SIGKILL`s the child, while the doc comment says a candidate only once there are no active or queued transfers; nothing implements that (`TransferManager::is_idle` is unused outside tests and is not on the wire). Inert today because nothing marks the slot resident; fix before wiring it up.

### `about:downloads`, the MCP adapter and the frontend (M4/M5 owned)

- [ ] **The refresh push breaks the "frame and `Representation` share one `generation`" invariant** (read). `send_frame` increments one counter shared by all tabs (`session.rs:643-646`) and `GetRepresentation` reports the current global value (`:238-241`), so with an `about:downloads` tab open and a download progressing, a client that gets `FrameReady(gen n)` for tab X can then be told `gen n+1`. Side effects: `shm::write_frame` keeps only 4 generations (`ipc/src/shm.rs:42`), so the MCP adapter's remembered `last_frame` for another tab is deleted after about 2 s and `screenshot` fails (`server.rs:302-308`); and `CoreConnection::record_frame` (`lib.rs:113-118`) sets `last_seen_tab` from unsolicited frames, so `screenshot` without `tab_id` can capture the human's downloads tab. The root cause predates M5 but M5 turned it into a constant stream. *Test first*: two tabs, one on `about:downloads` with a progressing download. Node IDs are also freshly allocated on every refresh, so IDs from an earlier snapshot of that tab go stale about every 500 ms; decide whether that is acceptable and say so in `PLAN.md`.
- [ ] **The `rendered` cache goes stale after a same-URL navigation** (read). `session.rs:412` skips re-rendering when the HTML equals `rendered[tab]`, which is cleared only on a URL change or on leaving the page (`:374`, `:382`). If a tab already on `about:downloads` is navigated to it again and the 300 ms `fetch_quick` (`page.rs:125`) times out, the page shows "service not running" and stays that way until the list changes. *Fix*: clear the cache on any navigation to the page.
- [ ] **Transient stalls flip the page to "not running"** (read): any read error, including one 5 s stall, maps to the `Unavailable` view (`session.rs:407-410`); and `fetch_within`'s helper thread (`downloads_page.rs:327`) can leak one thread and fd per poll against a peer that sends half a frame, because the framing layer retries timeouts once bytes have arrived. Debounce the unavailable view, and bound the helper.
- [ ] **The MCP downloads client has no timeouts** (read): `DownloadsHandle::call` (`mcp-server/src/downloads.rs:257-278`) holds the `client` mutex for the whole call and sets no socket timeouts, so a hung process, or `pause_transfer` waiting up to its 15 s settle timeout, blocks every other download tool. Also, the first non-idempotent call after the downloads process restarted or was idle-torn-down fails with "not responding" though nothing was sent (`Io` is not split into write versus read failure); the next call succeeds.
- [ ] **`frontend-reference` `download <url>`** (read): `wait_for_socket` only checks `path.exists()` (`main.rs:96-105`), so after a crash leaves a stale socket file it returns true at once and `connect` fails; poll `connect` as `DownloadsHandle` and `DownloadsSource` do. It also spawns the child with inherited stdio (`main.rs:354`) where the other spawners null them.

### Test debt (found by re-reading the suite, not by coverage)

- [ ] Missing cases behind the P1 items: empty file at manager level, more than one simultaneous stall, case-only-different destinations, resuming a failed transfer whose destination is now claimed, a dangling-symlink `dest`, and bidi/invisible file names.
- [ ] `every_transfer_result_is_framed_as_untrusted_data` (`mcp-server/tests/mcp_downloads.rs:448-455`) is weak: it skips error results ("our own text", but `start` errors echo the agent-supplied URL and `dest`), `hostile_name > framing_ends_at` is trivially true from JSON ordering, and it never checks a hostile `last_error` or `blocked.reason`, the list result's content, or a name containing the marker.
- [ ] Stale workarounds: `mcp_downloads.rs:363` ("core does not yet resolve a relative href") and `:372` ("navigate can return before the page has loaded") survive `9ecc1ea`, which fixed both, so nothing end to end proves either fix.
- [ ] Timing and hygiene: `session.rs`'s `about:downloads` tests use fixed settle loops (1200 ms), a 300 ms sleep, a 1600 ms negative window and wall-clock bounds (<3 s, <2 s), which are CI-load-sensitive; several "never auto-started" checks are `sleep(300 ms)` negative assertions (`protocol.rs:399`, `:571`, `:609`, `:839`; `downloads_binary.rs:211`); `a_subscriber_that_never_reads_cannot_stall_other_clients` (`protocol.rs:802`) never proves the socket buffer filled and asserts only "under 20 s"; the `core_binary.rs` `about:downloads` test has no cleanup guard, so a failed assertion leaves `blueice-core` and a detached `blueice-downloads` running; `a_link_to_about_downloads_is_followed_like_any_built_in_page` (`session.rs:2186`) uses `OpenTab`, not a link.
- [ ] The assertion left in `ai_snapshot.rs:400` after `2895680` removed a vacuous one is still weak: it asserts only that `nodes[0].id` resolves in the document, and DOM IDs are small sequential integers, so compare against the actual `<p>` node's id instead.
- [ ] The M6 "test-review pass" was mostly coverage-gap filling and did not catch any of the above; do the next one by reading assertions, not the coverage report. The 96.7% total-coverage figure recorded for M6 was **not** re-verified by the audit (`cargo llvm-cov` could not run: `llvm-tools-preview` was missing).

### Cross-phase (owned elsewhere; listed because they block this phase's Definition of Done)

- [ ] **`cargo test --workspace` does not terminate at HEAD.** `blueice-mcp-server --lib` deadlocks in `navigate_addresses_the_given_tab_id_on_the_wire` (`mcp-server/src/lib.rs:1266`): its fake core replies only `Navigated`, while `navigate` (changed in `9ecc1ea`, Phase 12) now also waits for `FrameReady`, so each side waits on the other. Every other test binary passed. M6's green `cargo test --workspace` was true at `d9ac454` and is no longer true. Related, same function: `navigate` has no read timeout and `blueice_net::fetch` has no request timeout, so a server that accepts and never answers can hang `navigate` and every browsing tool behind it; tracked with Phase 12 ([`../phase-12-mcp-server/PLAN.md`](../phase-12-mcp-server/PLAN.md)), and its own findings there are not repeated here.
- [ ] **Phase 11 backend findings** (an FTP final `226` error that is dropped, SFTP running segmented while the docs say single-stream, FTP passive-mode addresses trusted, secrets passing through MCP tool arguments and a `Debug`-derived request type, and the untested host-key / TLS behavior) belong to [`../phase-11-transfer-protocol-clients/PLAN.md`](../phase-11-transfer-protocol-clients/PLAN.md); they are not tracked in this file.
- [ ] **Documentation sync for this phase**: `PLAN.md:37` says a stalled thread "can never affect correctness" (true, but wrong for liveness, see the P1 item); `PLAN.md:102` says the clearance binds "the requested URL" (see the `final_url` item); `PLAN.md:148` does not say later segment requests follow unreviewed redirects. `CLAUDE.md` and `PLAN.md` still say the MCP adapter has "seven" download tools; at `5932f5d` it has 13 download-related tools (the seven plus the Phase 11 credential tools).

---

## 4. Known risks and unverified items

- **`ureq` has no per-`read` timeout.** Every `timeout_*` setting I found is a whole-phase timeout, which does not suit long downloads. Stall detection therefore lives in the coordinator's watchdog: it revokes the stalled segment's owner and reassigns it. A thread blocked in a `read()` can, in the worst case, survive until the TCP layer reports an error; it cannot affect data correctness. **It can affect liveness**, though: the stuck thread keeps its worker slot, so after `max_connections` stalls no replacement can start and the transfer hangs (see the P1 item in §3).
- **`ureq` body-streaming details are unconfirmed.** Whether `into_reader` has no size limit is not yet verified; my first source lookup used the wrong path. Verifying this is the first step of M2.
- **Percentage widths in layout** are unverified (D13).
- **Sparse files.** Pre-allocating with `set_len` does not fail on a full disk until bytes are written; this is covered under error handling.
- **The hand-rolled HTTP test server** determines how far every engine test can be trusted, so it gets its own basic tests first.
- **Wire-level enforcement.** A token does not cross a process boundary. This slice has the downloads process query the gatekeeper itself and not trust its callers (the MCP server included). IPC-level enforcement remains the existing open item from Phase 9 and is not solved here.
- **`ai-gatekeeper` is currently an always-clear stub**, so the "blocked" path can only be tested with a fake gatekeeper; it does not exercise real review content.

## 5. Requests for the reviewer

1. Should any of D1–D15 change? The ⚠ rows (D1, D2, D4, D6, D7, D10, D11, D14) most need a look.
2. Should the scope or the order of the milestones change?

This file lives in `development/browser_core/phase-10-download-manager/`, next to this phase's [`PLAN.md`](PLAN.md). `PLAN.md`'s own checklist is phase-level; this file is the working list for the first slice, and M0 and M6 write the conclusions back into `PLAN.md`.
