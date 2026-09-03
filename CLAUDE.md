# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

BlueIce is pre-implementation: there is no source code, no `Cargo.toml`, and no build/lint/test tooling yet. Everything currently in the repo is design documentation under `development/`. When implementation begins, update this file with the actual build/lint/test commands (this is a Rust project).

## Core goal

BlueIce is a browser engine, written from scratch in Rust, built around one idea: a human user and an AI agent should perceive the same page state from the **same render pass**, instead of the common dual-track setup where a human uses a real browser and an AI drives a separate headless instance via external automation (e.g. Puppeteer/CDP). That dual-track approach is rejected for architectural reasons, not just performance:

- Anti-bot systems (Cloudflare, Akamai, PerimeterX) fingerprint headless/CDP Chromium and can serve it different content than a human sees.
- A separate driven instance has its own viewport/timing/JS state, so "what the AI saw" is only ever an approximation of "what the human saw."
- Any AI-facing capability (e.g. semantic tags alongside human-facing highlights) has to be a post-hoc hack in an externally-driven architecture; owning the render pipeline makes it a first-class feature instead.

This is not a clean-room implementation — Gecko (Firefox) and Blink/V8 (Chromium) source are read directly as technical reference and porting basis. See `development/browser_core/BROWSER_CORE_PLAN.md` for full detail.

## Design-first workflow

Plans are drafted under `development/` *before* implementation starts, and updated as the design evolves — treat these as living documents, not historical records. Each major component gets its own subdirectory with a plan doc. Currently the only component is `development/browser_core/BROWSER_CORE_PLAN.md` (the engine itself).

**Open decision to check before assuming an approach**: which layer the human and AI representations are shared at is not yet settled (plan §3). Candidates under consideration: raw pixels + OCR/vision model, DOM+CSSOM, the accessibility tree, or a custom hybrid derived from one render pass. Don't assume one of these has been chosen without checking the plan doc.

**MVP scope** (per plan §4) is deliberately narrow: a minimal HTML parse → DOM → CSS cascade → layout → paint pipeline, plus the shared human/AI representation layer. Explicitly out of scope for now: extension ecosystem, multi-tab state sync, full JS engine optimization, DevTools.

## Licensing (load-bearing, not boilerplate)

Project-wide license is MPL-2.0, chosen specifically because the project reads and adapts Gecko/Chromium source rather than clean-rooming:

- Files derived from Gecko **must** stay MPL-2.0 — this is an inherent MPL file-level copyleft obligation, not a project choice.
- Files derived from Chromium/Blink (BSD-3-Clause) are relicensed to MPL-2.0, but the original BSD copyright/disclaimer notice must be preserved (file header or a centralized `NOTICE`/`THIRD_PARTY_LICENSES` file — neither exists yet).
- Wholly original code (the AI representation layer, project glue) uses MPL-2.0 for consistency only, with no derivative-work obligation.
- The license places no field-of-use/commercial restriction (that would violate the OSI Open Source Definition) — "no commercial activity" is a project stance, not something encoded in the license terms.
- MPL §2.1's patent grant only covers contributors' own contributions to this project, not unrelated third-party patents (codecs, GPU rendering, JIT techniques). This risk doesn't disappear at non-commercial stage.
- Do not use "Firefox", "Mozilla", "Chrome", "Google Chrome", or "Chromium" names/logos as BlueIce branding. Descriptive references ("BlueIce references Gecko's approach to X") are fine as nominative fair use.

## Naming

The brand name is written **BlueIce** (capital I) in prose and docs. The lowercase `blueice` is reserved for technical identifiers only — the GitHub repo slug (`ephoton0210/blueice`) and filesystem paths — not for prose.
