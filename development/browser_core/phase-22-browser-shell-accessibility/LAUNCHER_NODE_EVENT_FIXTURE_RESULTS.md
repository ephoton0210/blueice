# Launcher click fixture after node-event implementation

Date: 2026-10-13. The complete macOS `ec4217440` workspace gate
ends with 7,603 passed, three failed and 69 ignored. All 1,902
frozen program inputs match. Two failures are the already corrected
Array.fromAsync and Atomics.waitAsync GC regressions; the third is the
launcher click fixture's obsolete event-API assumption.

The fixture throws before installing its JavaScript listener whenever
`dispatchEvent` exists. Node EventTarget dispatch is now implemented, so
that preflight contradicts the current API and the later click cannot
prevent navigation or schedule its DOM mutation. The fixture now requires
a callable dispatch method. All native JS/BlueTS click, cancellation,
throw containment, Promise-job DOM effect, created subtree and three
FrameReady assertions remain. Unexpected frame replies are included in
assertion diagnostics.

The original fixture fails against both original and fixed production
binaries. The corrected fixture passes all seven Core/child integration
cases against the same fixed `504eeb924` binaries with zero failures or
skips. An independent Rust test executable links existing immutable
libraries and uses sibling symlinks to the eight fixed binaries; no
running Cargo/native cache or production source is changed. Those eight
binary hashes are unchanged after the gate. Rustfmt and diff checks pass.
The [receipt](artifacts/launcher-node-event-fixture-results.json) records
commands, same-runtime red/green results and hashes.

This Unix-only test change does not affect Windows or native product
behavior. Canonical full corrected macOS Rust validation remains required.
The [complete failed Rust receipt](artifacts/macos-workspace-ec4217440-failed-results.json)
retains the original three failures and source snapshot.
