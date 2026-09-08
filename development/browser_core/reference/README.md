## Reference source checkouts

Shallow, read-only clones of the upstream projects BlueIce reads directly as technical reference and a porting basis (plan §4). These are third-party source, not BlueIce's own code — `gecko/`, `chromium/`, and `v8/` are gitignored and never committed; only specific files actually ported out of them (with the header conventions from [`CONTRIBUTING.md`](../../../CONTRIBUTING.md)) become part of this repo.

Findings from reading this source belong in [`../research/`](../research/) as durable, reviewable notes — treat this directory as disposable, re-fetchable raw material, not a place to leave conclusions.

### Fetching

```sh
git clone --depth 1 https://github.com/mozilla-firefox/firefox.git gecko
git clone --depth 1 https://github.com/chromium/chromium.git chromium
git clone --depth 1 https://github.com/v8/v8.git v8
```

**`wpt/` (the WPT/html5lib-tests tree-construction corpus) is fetched differently** -- it's a small subdirectory of an enormous monorepo, so a sparse partial clone keeps it to tens of megabytes instead of attempting a full checkout of `web-platform-tests/wpt`:

```sh
git clone --filter=blob:none --sparse --depth 1 https://github.com/web-platform-tests/wpt.git
cd wpt && git sparse-checkout set html/syntax/parsing
```

The tree-construction `.dat` files (the `#data`/`#errors`/`#document`/`#document-fragment` format `blueice-testing`'s fixture parser already reads unmodified) live under `wpt/html/syntax/parsing/resources/`. Historically these lived in the standalone `html5lib/html5lib-tests` repo; that repo's own `README.md` now says they moved here. Used by `backend/core/html/tests/wpt_corpus.rs` -- see that file's module docs and `testing/TEST_PLAN.md`'s "WPT tree-construction corpus" section for how it's run and what it found.

**`v8/` is separate from `chromium/` on purpose.** Chromium's own repo doesn't contain V8's source directly — Chromium pulls it in via its `DEPS` file (a `gclient sync`-managed external, pinned to a specific commit), which a plain `git clone` of the Chromium repo does not fetch. V8 develops in its own repository, so it's cloned independently here rather than expected to appear under `chromium/v8/`. (The same is true of several other Chromium dependencies — e.g. Skia, ANGLE — if research ever needs one of those, clone it the same way rather than looking for it inside `chromium/`.)

Both Gecko and Chromium clones are official read-only mirrors of the canonical upstream source; V8's is the project's own canonical repo directly. `--depth 1` gets the current source tree without full history — enough for reading and porting. If history archaeology is ever needed for a specific file (e.g. understanding why a workaround was added), deepen just that file's history rather than re-cloning fully:

```sh
git log --follow -p -- path/to/file.cc   # after: git fetch --unshallow
```

### Updating

These are point-in-time snapshots, not kept continuously in sync. Re-run the clone commands above (removing the old directory first) when a fresh snapshot is actually needed — there's no expectation of tracking upstream HEAD as it moves.
