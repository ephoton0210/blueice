# Testing strategy

[← Back to plan](../BROWSER_CORE_PLAN.md)

What gets tested, how, and where a human can see current status without reading source. This is a living policy, not a one-time checklist — every phase's Definition of Done includes keeping this accurate, not just this document's own author.

## Definition of Done

Every phase's checklist in `../phase-*/PLAN.md` carries the following as part of each checklist item — not satisfied by a design being settled or code merely compiling:

- **TDD is the development process, not just an outcome.** Write the test first, watch it fail for the right reason, then write the implementation that makes it pass. Tests bolted on after a feature is already fully written are the exception to reach for on legacy code, not the default way to build something new here.
- **Passing tests, not just compiling code.** A settled design, or a function that builds, is not "done" until tests pass against it.
- **An end-to-end test through the feature's real public interface, wherever an end-to-end path exists.** Test the crate's actual public API, not only the internal modules underneath it — an internal-only test can stay green while the feature itself is broken or unreachable from outside. `blueice-html` is the current reference: `backend/core/html/tests/parsing.rs` drives the parser only through `blueice_html::parse` — the same entry point every real caller uses (today `blueice-engine`; later `css`/`layout`) — never through `tokenizer`/`tree_builder` directly. Once `html`→`css`→`layout`→`paint` are all real, this is what "Rendering-correctness fixtures" below graduates into: a full-pipeline entry point, exercised the same way.
- **A complete public interface for tests to drive.** If exercising a feature requires a test to reach into `pub(crate)` or module-private internals, that's a gap in the interface, not something for the test to work around — complete the public surface rather than add test-only backdoors.
- **≥90% line coverage**, per "Coverage policy" below — a feature under the floor isn't finished, whatever else it does.
- **A dedicated test-review pass once the implementation itself is otherwise complete.** TDD's red-green cycle and the coverage floor both guard against *missing* tests, not *wrong* or *stale* ones — a test written first can still end up asserting the wrong thing once the implementation settles, and a test that was correct when written can go stale as the code around it changes. Before calling the checklist item done, re-read the suite's actual content (not just whether it's green or what percentage it reports) and: (a) add cases the incremental TDD cycles didn't happen to surface — edge cases, error paths, interactions between pieces written in separate cycles; (b) update any test whose expectation no longer matches the feature's actual intended behavior; (c) delete tests that no longer exercise anything meaningful. A stale test left passing for the wrong reason is worse than no test — it reads as coverage that isn't real. This review is part of the same checklist item, not a follow-up task to schedule later.

## Test pyramid

- **Unit tests**: per-crate, colocated `#[cfg(test)] mod tests` (standard Rust convention), testing one function/type's behavior in isolation. `blueice-dom` and `blueice-html` (`backend/core/html/src/{tokenizer,tree_builder}.rs`) are the current reference examples.
- **Integration/fixture tests**: cross-crate, exercising the real pipeline end to end (e.g. Phase 3's planned "fixture HTML+CSS → asserted paint output" smoke test). These belong in each crate's `tests/` directory once there's a real pipeline to assert against. `blueice-html` has a first one (`backend/core/html/tests/parsing.rs`, against the public `parse` API rather than internal modules) — `css`/`layout`/`paint` are still stubs, so the full-pipeline version of this waits on them.
- **Rendering-correctness fixtures**: once `html`→`css`→`layout`→`paint` are real, the actual functional backbone of a browser engine is a corpus of small HTML/CSS pages checked against expected output (DOM shape, computed styles, layout geometry) — closer to how WPT/reftests work than to unit tests. This category doesn't exist yet either; flagging it now so it's planned for, not retrofitted once the pipeline exists.
- **UI tests**: not yet actionable — there is no frontend (Phase 4 hasn't started). See "UI testing strategy" below for the plan; treat any claim of current UI test coverage before Phase 4 as wrong.
- **Differential/cross-validation tests** (BlueJS-specific): correctness and performance of the custom JS engine (Phase 13) checked against Node.js as a reference oracle, rather than trusted to BlueJS's own test suite alone — run the same script corpus through `node` and through the `bluejs` shell's batch mode, diff stdout/exit codes (correctness, gates the build for in-scope features) and wall-clock time (performance, tracked as a trend, not a pass/fail threshold — parity with Node/V8 isn't the MVP bar). See `../phase-13-bluejs-engine/PLAN.md` for the full design, including the non-determinism-normalization traps this category has to account for.

## Coverage policy

Tool: [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) (LLVM source-based coverage — more accurate than instrumentation-based tools like tarpaulin, no nightly toolchain required).

- **Crates with real implementation** must hold **≥90% line coverage**, enforced in CI via `--fail-under-lines 90`. Currently `blueice-dom` (98%+) and `blueice-html` (92%+ tokenizer/tree-builder, 100% on the crate's public entry point).
- **Stub crates** (`css`, `layout`, `paint`, `engine`, `extension`, `ipc` — anything whose body is `todo!()`) are excluded from the enforced number via `--ignore-filename-regex`, since a coverage percentage on unreachable stub code is meaningless. They're still measured and reported every run for visibility, so nobody can quietly land real logic into a "stub" crate without a test gap showing up.
- **When a stub crate gets its first real implementation, removing it from the ignore-filename-regex list is part of that crate's own Definition of Done** — tracked as a checklist item in whichever phase implements it (currently Phase 3 for `css`/`layout`/`paint`/`engine`; `html` already did this).
- Coverage is necessary, not sufficient: a function hit by one happy-path test can show 100% line coverage while missing every edge case. Treat the percentage as a floor that catches accidentally-untested code, not a target to chase for its own sake — reason about what a crate's tests actually need to cover (per the pyramid above) independent of the number.

Run it locally the same way CI does:

```sh
cargo llvm-cov --workspace \
  --ignore-filename-regex '(core/(engine|css|layout|paint)/src/lib\.rs|extension/src/main\.rs)$' \
  --fail-under-lines 90 \
  --summary-only
```

## CI — the automation + human-visible interface

`.github/workflows/ci.yml` runs on every push and pull request to `main`:

- **`build-test`**: `cargo build --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- **`coverage`**: the `cargo-llvm-cov` command above, with its summary written to the job's step summary so the numbers are visible directly on the run page, not buried in a log.
- **`bluejs-differential`** (not added yet — part of Phase 13): will need Node.js in the CI environment (`actions/setup-node`) to run the differential corpus against `bluejs`, with correctness gating the build and performance numbers published to the job summary the same way coverage is.

This is the interface: GitHub's own Checks UI shows pass/fail per commit and per PR with no extra dashboard to build or maintain; the coverage job's summary shows real numbers on every run; the README badge links straight to the latest run so current status is visible from the project's front page. A human can also just run the same two commands locally (`cargo test --workspace`, the `cargo llvm-cov` command above) to get the identical result CI would.

## UI testing strategy (Phase 4+, not yet actionable)

Once a frontend exists, two layers, only the first of which is meaningfully automatable across all platforms:

1. **Core↔frontend IPC-boundary tests**: drive the control-plane protocol (plan §1, Phase 4/5) directly with a test client, without needing real platform UI automation. This should be the primary correctness net, since it runs the same on every CI platform regardless of which native frontend is under test.
2. **Platform-native UI automation**, one per real frontend, scoped to what's genuinely platform-specific (does the window actually appear and respond, does native accessibility/menu integration work) rather than re-testing logic layer 1 already covers: WinAppDriver (or successor) for WinUI 3, XCUITest for SwiftUI, `pytest-qt`/`QTest` for PySide6.

Neither layer exists yet. Updating this section with what's actually built is part of Phase 4's Definition of Done, not a separate later task.

## Cross-reference

See "Definition of Done" at the top of this document — it, not a separate end-of-phase pass, is what every phase checklist item in `../phase-*/PLAN.md` is actually held to.
