# macOS Test262 status

## Current status (2026-09-20)

**Deferred — no macOS test host is available in this environment.** The previous figures in this file described an older engine snapshot and are not valid evidence for the current source tree. They are intentionally not carried forward as a current conformance result.

The authoritative current measurement is Ubuntu-only and is recorded in the [Ubuntu Test262 report](TEST262_LINUX_REPORT.md). In particular, its ECMA-262, ECMA-402 and Temporal rates must not be assumed to reproduce on macOS.

## Required macOS rerun

On an available macOS host, build the checked-out revision and run:

```sh
python3 backend/bluejs/test262/run.py --jobs 8 --output /tmp/blueice-test262-macos
python3 backend/bluejs/test262/analyze.py --run /tmp/blueice-test262-macos \
  --output /tmp/blueice-test262-macos-analysis
```

Record the macOS release, architecture, Rust/Cargo versions, logical CPU count, Test262 snapshot revision, adapter SHA-256, elapsed time and every outcome category. Derive ECMA-262 Core (`language/` + `built-ins/`), complete ECMA-262 scope (Core + `annexB/` + `staging/`), ECMA-402 (`intl402/`), and the combined Temporal total from that one unfiltered JSONL inventory. Do not substitute a filtered run or Ubuntu result.
