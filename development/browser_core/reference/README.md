## Reference source checkouts

Shallow, read-only clones of the upstream projects BlueIce reads directly as technical reference and a porting basis (plan §4). These are third-party source, not BlueIce's own code — `gecko/`, `chromium/`, and `v8/` are gitignored and never committed; only specific files actually ported out of them (with the header conventions from [`CONTRIBUTING.md`](../../../CONTRIBUTING.md)) become part of this repo.

Findings from reading this source belong in [`../research/`](../research/) as durable, reviewable notes — treat this directory as disposable, re-fetchable raw material, not a place to leave conclusions.

### Fetching

```sh
git clone --depth 1 https://github.com/mozilla-firefox/firefox.git gecko
git clone --depth 1 https://github.com/chromium/chromium.git chromium
git clone --depth 1 https://github.com/v8/v8.git v8
```

**`test262/` (the pinned Test262 corpus) is required to compile `blueice-bluejs`'s tests.** Several integration tests (`tests/intl.rs`, `tests/try_completion.rs`, `tests/test262_host/modules.rs`) `include_str!` harness and fixture files from it, so without it `cargo test -p blueice-bluejs` fails at compile time with "couldn't read .../reference/test262/...". It is gitignored, and it is also the runner's default `--corpus` (`backend/bluejs/test262/run.py`). CI checks out `tc39/test262` at the revision pinned in `backend/bluejs/test262/snapshot.json`; do the same locally:

```sh
git clone https://github.com/tc39/test262.git test262
git -C test262 checkout 72faf8ec1445c55149615e8b35187830783aba1a
```

`python3 backend/bluejs/test262/run.py --fetch --corpus <dir>` instead downloads and manifest-verifies the same revision into `<dir>` (copy or symlink that directory here to reuse it). `cldr-json/` (also gitignored, pinned in CI) is only the input for the CLDR-provider reproducibility check, not for compiling tests.

**`wpt/` (the WPT/html5lib-tests tree-construction corpus) is fetched differently** -- it's a small subdirectory of an enormous monorepo, so a sparse partial clone keeps it to tens of megabytes instead of attempting a full checkout of `web-platform-tests/wpt`:

```sh
git clone --filter=blob:none --sparse --depth 1 https://github.com/web-platform-tests/wpt.git
cd wpt && git sparse-checkout set html/syntax/parsing css/css-color css/support fonts
```

The tree-construction `.dat` files (the `#data`/`#errors`/`#document`/`#document-fragment` format `blueice-testing`'s fixture parser already reads unmodified) live under `wpt/html/syntax/parsing/resources/`. Historically these lived in the standalone `html5lib/html5lib-tests` repo; that repo's own `README.md` now says they moved here. Used by `backend/core/html/tests/wpt_corpus.rs` -- see that file's module docs and `testing/TEST_PLAN.md`'s "WPT tree-construction corpus" section for how it's run and what it found.

`wpt/css/css-color/` is the pilot corpus for `phase-27-css-wpt-conformance/PLAN.md` -- real WPT CSS reftests (`<link rel="match"|"mismatch" href="...">`), compared by rendering both the test and its reference through BlueIce's own pipeline rather than against a second engine (see that phase's own doc for why this needs no Puppeteer/Chromium dependency, unlike Phase 15). `css/support/` and `fonts/` are shared resources a handful of those reftests reference by root-absolute path (`/css/support/...`, `/fonts/ahem.css`); widen the `sparse-checkout set` line above to add another CSS suite directory (e.g. `css/css-backgrounds`) the same way, rather than a separate clone.

**`v8/` is separate from `chromium/` on purpose.** Chromium's own repo doesn't contain V8's source directly — Chromium pulls it in via its `DEPS` file (a `gclient sync`-managed external, pinned to a specific commit), which a plain `git clone` of the Chromium repo does not fetch. V8 develops in its own repository, so it's cloned independently here rather than expected to appear under `chromium/v8/`. (The same is true of several other Chromium dependencies — e.g. Skia, ANGLE — if research ever needs one of those, clone it the same way rather than looking for it inside `chromium/`.)

Both Gecko and Chromium clones are official read-only mirrors of the canonical upstream source; V8's is the project's own canonical repo directly. `--depth 1` gets the current source tree without full history — enough for reading and porting. If history archaeology is ever needed for a specific file (e.g. understanding why a workaround was added), deepen just that file's history rather than re-cloning fully:

```sh
git log --follow -p -- path/to/file.cc   # after: git fetch --unshallow
```

### Updating

These are point-in-time snapshots, not kept continuously in sync. Re-run the clone commands above (removing the old directory first) when a fresh snapshot is actually needed — there's no expectation of tracking upstream HEAD as it moves.
