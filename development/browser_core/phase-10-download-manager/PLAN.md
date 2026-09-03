# Phase 10 — Built-in Download Manager

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

## Objective

A full built-in file-transfer subsystem: chunked, resumable, multi-threaded downloads, comparable in capability to Free Download Manager — not just "save this HTTP response to disk."

## Design sketch

**Process design**: `backend/downloads/` as its own isolated OS process, a peer to `extension` and `ai-local` rather than a child of either, speaking the same IPC family as the rest of plan §1's architecture.

**Chunking algorithm** (the well-understood part, per the objective above — sketched here for concreteness, not because it's in doubt): `HEAD` the resource to get `Content-Length` and confirm `Accept-Ranges: bytes`; if supported, split into N concurrent `Range: bytes=start-end` requests via `reqwest`, writing each chunk directly into its byte range of a pre-allocated destination file (`File::set_len` + positioned writes — avoids N separate temp files that need stitching together afterward); fall back to a single sequential stream when range requests aren't supported.

**Resume**: a small sidecar metadata file next to the in-progress download (URL, total size, per-chunk completed-byte ranges, `ETag`/`Last-Modified`). On resume, validate the remote resource is unchanged (matching `ETag`/`Last-Modified`) before continuing byte-for-byte — restart from scratch if it's changed, rather than silently splicing old and new content together.

**State/UI surface**: the process exposes a `Transfer { id, url, dest_path, total_bytes, completed_bytes, state }` list over IPC (`state` ∈ Queued/Active/Paused/Completed/Failed), with `frontend` subscribing to push updates for its downloads UI rather than polling.

## Open questions

- ~~Process placement~~ — **isolated process, confirmed**, consistent with `extension`'s isolation rationale (plan §1). [`research/multi-process-memory.md`](../research/multi-process-memory.md) additionally flags `downloads` as an idle-teardown candidate (no reason to stay resident with zero active/queued transfers) — the Phase 8 launcher is the proposed owner of that teardown authority once it exists.
- **Resume-across-restart**: chunked/parallel resume within one browser session is a well-understood technique (HTTP range requests, as aria2/FDM/IDM already do — not really an open design question). What's not decided is whether in-progress transfer state persists to disk so a download can resume after the *browser itself* restarts, not just after a network blip, and if so, where that state lives.
- **Relationship to Phase 11** (FTP/SFTP clients): same transfer-manager subsystem with pluggable protocol backends behind one interface, or a fully separate component? Affects how both phases are scoped.

## Checklist

- [ ] Decide process placement (isolated process vs. in-`core`)
- [ ] Decide whether transfer state persists across browser restarts, and where
- [ ] Decide the relationship to Phase 11 (shared subsystem vs. separate)
- [ ] Design the chunked/parallel/resumable transfer protocol (HTTP range-request based)
- [ ] Evaluate `reqwest` as the HTTP client base
- [ ] Define the file-management surface (where downloads land, in-progress/completed/failed state visible to `frontend`)
