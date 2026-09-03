## Reference source checkouts

Shallow, read-only clones of the upstream projects BlueIce reads directly as technical reference and a porting basis (plan §4). These are third-party source, not BlueIce's own code — `gecko/`, `chromium/`, and `v8/` are gitignored and never committed; only specific files actually ported out of them (with the header conventions from [`CONTRIBUTING.md`](../../../CONTRIBUTING.md)) become part of this repo.

Findings from reading this source belong in [`../research/`](../research/) as durable, reviewable notes — treat this directory as disposable, re-fetchable raw material, not a place to leave conclusions.

### Fetching

```sh
git clone --depth 1 https://github.com/mozilla-firefox/firefox.git gecko
git clone --depth 1 https://github.com/chromium/chromium.git chromium
git clone --depth 1 https://github.com/v8/v8.git v8
```

**`v8/` is separate from `chromium/` on purpose.** Chromium's own repo doesn't contain V8's source directly — Chromium pulls it in via its `DEPS` file (a `gclient sync`-managed external, pinned to a specific commit), which a plain `git clone` of the Chromium repo does not fetch. V8 develops in its own repository, so it's cloned independently here rather than expected to appear under `chromium/v8/`. (The same is true of several other Chromium dependencies — e.g. Skia, ANGLE — if research ever needs one of those, clone it the same way rather than looking for it inside `chromium/`.)

Both Gecko and Chromium clones are official read-only mirrors of the canonical upstream source; V8's is the project's own canonical repo directly. `--depth 1` gets the current source tree without full history — enough for reading and porting. If history archaeology is ever needed for a specific file (e.g. understanding why a workaround was added), deepen just that file's history rather than re-cloning fully:

```sh
git log --follow -p -- path/to/file.cc   # after: git fetch --unshallow
```

### Updating

These are point-in-time snapshots, not kept continuously in sync. Re-run the clone commands above (removing the old directory first) when a fresh snapshot is actually needed — there's no expectation of tracking upstream HEAD as it moves.
