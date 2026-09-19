# Windows Test262 status

## Current status (2026-09-20)

**Deferred — no Windows test host is available in this environment.** The previous figures in this file were produced by an older engine snapshot and are not current conformance evidence. They are intentionally removed rather than being compared with the current Ubuntu result.

The authoritative current measurement is Ubuntu-only and is recorded in the [Ubuntu Test262 report](TEST262_LINUX_REPORT.md). Its ECMA-262, ECMA-402 and Temporal outcomes establish neither Windows parity nor a Windows timeout profile.

## Required Windows rerun

On an available Windows host, build the checked-out revision and run:

```powershell
python backend/bluejs/test262/run.py --jobs 8 --output $env:TEMP\blueice-test262-windows
python backend/bluejs/test262/analyze.py --run $env:TEMP\blueice-test262-windows `
  --output $env:TEMP\blueice-test262-windows-analysis
```

Record the Windows release, architecture, Rust/Cargo versions, logical CPU count, Test262 snapshot revision, adapter SHA-256, elapsed time and every outcome category. Derive ECMA-262 Core (`language/` + `built-ins/`), complete ECMA-262 scope (Core + `annexB/` + `staging/`), ECMA-402 (`intl402/`), and the combined Temporal total from that one unfiltered JSONL inventory. Include `.exe` helper-hash verification in the report; do not substitute a filtered run or Ubuntu result.
