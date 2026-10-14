# macOS page-host final reply and EOF

Snapshot base: `0d2e3ea583f163c322f6d3a9f4f331a97bddb089`.

## Problem and correction

The integrated macOS workspace run failed in
`every_debugger_dispatch_method_forwards_its_reply_and_every_capability_flag_is_true`:
its final pumped request returned `page-host read timeout: Invalid argument`.
A standalone Rust 1.96 Unix-socket probe confirmed the OS behavior: after the
peer sends a complete byte and closes, setting a positive receive timeout can
return `EINVAL`, while reading the buffered byte still succeeds.

`PageHostConnection` now installs the receive timeout before writing the
request. The peer therefore cannot close in response to that request before
the receive bound exists. Reader cloning and reply parsing remain after the
write. The original absolute deadline still bounds the complete operation;
expiry shuts down the socket and joins the reader. No total wait budget or
validation threshold was increased.

## Verification

The focused transport group passed all 12 cases, including the previously
failed 45-method debugger dispatch test, 128 final-reply/immediate-close
exchanges, stalled-child timeout/shutdown, fixed nested-call budget and DOM
pumping on the owning session thread. The new 128-exchange case also passed
before the correction on the quiet test run; the original full-run failure and
the deterministic kernel probe establish the faulty interleaving. This record
does not claim that the race fails on every execution.

Focused log: `/private/tmp/blueice-page-host-transport-tests.log`.
The final integrated `cargo test --workspace` completed with 7,200 passed,
zero failed and 69 ignored across 468 result groups. The run includes the
upcoming native file-input increment; the focused socket tests exercise the
transport correction independently.

The 56-input integration snapshot has SHA-256
`d6d99628282dc5d06dd8ba720c1d527219dda72168965b4ba11e84e93b7b9a03`.
The workspace all-targets build, Clippy with `-D warnings`, rustfmt and whitespace
checks passed. Native file-input XCTest/UI tests passed four cases with the
corrected core in `frontend/macos/.build/results-20261003-121022.xcresult`.
All Rust commands reused `frontend/macos/.build/core-target`.
Final integration log: `/private/tmp/blueice-file-input-workspace-accepted-tests.log`.

No Linux/Windows runtime or fresh coverage result is claimed here. The native
app and GUI evidence are recorded by the subsequent file-input increment.
