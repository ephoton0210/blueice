# Repository Guidelines

## Project Structure & Module Organization

BlueIce is a Rust 2021 workspace. Browser-pipeline crates live in
`backend/core/` (`dom`, `html`, `css`, `layout`, `paint`, `raster`, and
`engine`); supporting processes and protocols live under `backend/` (for
example, `ipc`, `launcher`, `mcp-server`, and `bluejs`). Keep unit tests in
each crate's `tests/` directory and reusable fixtures beside them in
`tests/fixtures/`. Design decisions and phase plans belong in
`development/browser_core/`. The standalone Node/Puppeteer visual-comparison
harness is `differential-testing/`.

## Build, Test, and Development Commands

- `cargo build --workspace --all-targets` builds every Rust crate and target.
- `cargo test --workspace` runs the workspace test suite.
- `cargo clippy --workspace --all-targets -- -D warnings` treats all Clippy
  warnings as errors, matching CI.
- `cargo fmt --all -- --check` verifies Rust formatting; run `cargo fmt --all`
  before committing changes.
- In `differential-testing/`, `npm ci` installs the locked dependencies and
  `npm run differential` captures BlueIce and Chromium output, then compares
  them.

The HTML WPT corpus is fetched by CI into
`development/browser_core/reference/wpt`; follow that directory's README when
you need to run its full gate locally.

## Coding Style & Naming Conventions

Use standard `rustfmt` output (four-space indentation) and idiomatic Rust:
`snake_case` for functions/modules, `PascalCase` for types, and `SCREAMING_SNAKE_CASE`
for constants. Name crates and directories with lowercase hyphenated names.
Keep changes focused by crate and preserve existing fixture formats. Use
**BlueIce** in prose; reserve lowercase `blueice` for technical identifiers.

Every new source file needs the MPL-2.0 header from `CONTRIBUTING.md`. For
Gecko- or Chromium/Blink-derived code, include its required upstream
provenance and preserve the original Chromium notice verbatim.

## Testing Guidelines

Develop test-first and add regression coverage at the relevant public boundary.
CI requires at least 90% workspace line coverage; `blueice-bluejs` must retain
100%. Run focused tests during development, e.g.
`cargo test -p blueice-bluejs --test strings`, then the workspace suite before
opening a PR.

## Commit & Pull Request Guidelines

Recent commits use concise imperative subjects, such as `Implement BlueJS
bound functions and RegExp.escape`. Keep each commit scoped to one coherent
change. PRs should explain behavior and test coverage, link the relevant issue
or phase plan, update `development/` plans when design/status changes, and add
screenshots or differential-test artifacts for rendering changes.
