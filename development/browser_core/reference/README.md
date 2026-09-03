## Reference source checkouts

Shallow, read-only clones of the two upstream projects BlueIce reads
directly as technical reference and a porting basis (plan §4). These are
third-party source, not BlueIce's own code — `gecko/` and `chromium/` are
gitignored and never committed; only specific files actually ported out of
them (with the header conventions from [`CONTRIBUTING.md`](../../../CONTRIBUTING.md))
become part of this repo.

Findings from reading this source belong in
[`../research/`](../research/) as durable, reviewable notes — treat this
directory as disposable, re-fetchable raw material, not a place to leave
conclusions.

### Fetching

```sh
git clone --depth 1 https://github.com/mozilla-firefox/firefox.git gecko
git clone --depth 1 https://github.com/chromium/chromium.git chromium
```

Both are official read-only mirrors of the canonical upstream source.
`--depth 1` gets the current source tree without full history — enough for
reading and porting. If history archaeology is ever needed for a specific
file (e.g. understanding why a workaround was added), deepen just that
file's history rather than re-cloning fully:

```sh
git log --follow -p -- path/to/file.cc   # after: git fetch --unshallow
```

### Updating

These are point-in-time snapshots, not kept continuously in sync. Re-run
the clone commands above (removing the old directory first) when a fresh
snapshot is actually needed — there's no expectation of tracking upstream
HEAD as it moves.
