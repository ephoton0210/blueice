# Testing strategy

[← Back to plan](../BROWSER_CORE_PLAN.md)

What gets tested, how, and where a human can see current status without reading source. This is a living policy, not a one-time checklist — every phase's Definition of Done includes keeping this accurate, not just this document's own author.

## Test pyramid

- **Unit tests**: per-crate, colocated `#[cfg(test)] mod tests` (standard Rust convention), testing one function/type's behavior in isolation. `blueice-dom` is the current reference example — see `backend/core/dom/src/lib.rs`.
- **Integration/fixture tests**: cross-crate, exercising the real pipeline end to end (e.g. Phase 3's planned "fixture HTML+CSS → asserted paint output" smoke test). These belong in each crate's `tests/` directory once there's a real pipeline to assert against — not written yet, since `html`/`css`/`layout`/`paint` are still stubs.
- **Rendering-correctness fixtures**: once `html`→`css`→`layout`→`paint` are real, the actual functional backbone of a browser engine is a corpus of small HTML/CSS pages checked against expected output (DOM shape, computed styles, layout geometry) — closer to how WPT/reftests work than to unit tests. This category doesn't exist yet either; flagging it now so it's planned for, not retrofitted once the pipeline exists.
- **UI tests**: not yet actionable — there is no frontend (Phase 4 hasn't started). See "UI testing strategy" below for the plan; treat any claim of current UI test coverage before Phase 4 as wrong.

## Coverage policy

Tool: [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) (LLVM source-based coverage — more accurate than instrumentation-based tools like tarpaulin, no nightly toolchain required).

- **Crates with real implementation** must hold **≥90% line coverage**, enforced in CI via `--fail-under-lines 90`. Currently that's just `blueice-dom` (96%+ today).
- **Stub crates** (`html`, `css`, `layout`, `paint`, `engine`, `extension`, `ipc` — anything whose body is `todo!()`) are excluded from the enforced number via `--ignore-filename-regex`, since a coverage percentage on unreachable stub code is meaningless. They're still measured and reported every run for visibility, so nobody can quietly land real logic into a "stub" crate without a test gap showing up.
- **When a stub crate gets its first real implementation, removing it from the ignore-filename-regex list is part of that crate's own Definition of Done** — tracked as a checklist item in whichever phase implements it (currently Phase 3 for `html`/`css`/`layout`/`paint`/`engine`).
- Coverage is necessary, not sufficient: a function hit by one happy-path test can show 100% line coverage while missing every edge case. Treat the percentage as a floor that catches accidentally-untested code, not a target to chase for its own sake — reason about what a crate's tests actually need to cover (per the pyramid above) independent of the number.

Run it locally the same way CI does:

```sh
cargo llvm-cov --workspace \
  --ignore-filename-regex '(core/(engine|html|css|layout|paint)/src/lib\.rs|extension/src/main\.rs)$' \
  --fail-under-lines 90 \
  --summary-only
```

## CI — the automation + human-visible interface

`.github/workflows/ci.yml` runs on every push and pull request to `main`:

- **`build-test`**: `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- **`coverage`**: the `cargo-llvm-cov` command above, with its summary written to the job's step summary so the numbers are visible directly on the run page, not buried in a log.

This is the interface: GitHub's own Checks UI shows pass/fail per commit and per PR with no extra dashboard to build or maintain; the coverage job's summary shows real numbers on every run; the README badge links straight to the latest run so current status is visible from the project's front page. A human can also just run the same two commands locally (`cargo test --workspace`, the `cargo llvm-cov` command above) to get the identical result CI would.

## UI testing strategy (Phase 4+, not yet actionable)

Once a frontend exists, two layers, only the first of which is meaningfully automatable across all platforms:

1. **Core↔frontend IPC-boundary tests**: drive the control-plane protocol (plan §1, Phase 4/5) directly with a test client, without needing real platform UI automation. This should be the primary correctness net, since it runs the same on every CI platform regardless of which native frontend is under test.
2. **Platform-native UI automation**, one per real frontend, scoped to what's genuinely platform-specific (does the window actually appear and respond, does native accessibility/menu integration work) rather than re-testing logic layer 1 already covers: WinAppDriver (or successor) for WinUI 3, XCUITest for SwiftUI, `pytest-qt`/`QTest` for PySide6.

Neither layer exists yet. Updating this section with what's actually built is part of Phase 4's Definition of Done, not a separate later task.

## Cross-reference

Every phase's checklist in `../phase-*/PLAN.md` implicitly includes "add/update tests for what this phase built" and "real crates stay ≥90% line coverage" as part of each checklist item, not as a separate pass done at the end of the phase.
