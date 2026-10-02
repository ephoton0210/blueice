# macOS GET/POST form submission validation

Validated on 2026-10-02 with the SwiftUI/AppKit shell, bundled launcher, real
core and compiled gatekeeper. This extends form-reset milestone `4c1c6e922`;
the [delivery plan](MACOS_DELIVERY_PLAN.md) retains the full browser scope.

## Measured results

| Check | Result |
| --- | --- |
| Build, complete native suite and attachment export | Exit 0 |
| XCTest: 14 accessibility, 9 editing/keyboard, 14 protocol/process | 37 passed |
| Actual-window XCUITest | 20 passed, 0 failed, 1 physical IME case skipped |
| Native result summary | 58 total, 57 passed, 0 failed, 1 skipped |
| SwiftUI view-update state warnings | 0 |
| Internal XCTest QoS runtime warnings | 13 |
| Engine/net/IPC/HTML/layout/gatekeeper libraries and selected public/process boundaries | 1,269 passed |
| Public native form cases, included above | 15 passed |
| MCP library and all integration targets | 191 passed |
| Unique Rust cases | 1,460 passed, 0 failed in final successful runs |
| Existing manual local-model quality probe | 1 ignored; requires explicit live-model configuration |
| Workspace check, all targets, locked/offline | Passed |
| Clippy: 8 affected packages, all targets, warnings denied | Passed |
| Rust formatting, Git whitespace, Xcode project parsing | Passed |
| App and three bundled services: strict signature verification | Passed |
| Validation image and result-bundle Git ignore checks | Passed |
| Owned native bundle app/service processes after complete suite | 0 |

## Delivered behavior

The previous public session tests timed out without a form navigation. The
core now derives successful controls in document order, including external
form owners, repeated names, readonly fields, selected enabled options and the
actual submitter. Disabled controls/fieldsets, datalist descendants, unchecked
checkboxes/radios, unnamed controls and other submit buttons are excluded.
Submitter method/action/encoding overrides and the document base URL are honored.
The parser now treats base as a void head element instead of swallowing the
following form. Implicit Enter uses the default submitter; a single blocking
field without a submitter can submit without a synthesized form click.

GET replaces the action query and keeps data in the URL. POST keeps the action
query and sends the data in its private body. UTF-8 URL-encoded, text/plain and
multipart encodings normalize line endings; hidden _charset_ uses UTF-8.
File inputs currently represent an empty selection: a value attribute cannot
read an OS path. Encoded requests are bounded to one MiB and 1,024 entries.
Required checks include individual nameless radios, checkbox state, select
placeholder/multiple/listbox behavior and form/submitter validation opt-outs.
Default/clamped range values now match shared pixels, semantic values and
submission, while valid original numeric spellings preserve large integers.
The real native form fixture verifies both slider value 50 and submitted 50.

Every connection retains mandatory URL review; forms add document/action URL,
method and protected-field-presence review. Compiled policy excludes protected
fields from GET and requires same-origin HTTPS POST, including retained-body
307/308 redirects. POST 301/302 and all 303 responses become GET. Final content,
including an HTTP error page, is reviewed before parsing. POST bodies are
excluded from Debug, review metadata, semantic snapshots, extension network
traces and frontend messages. Ordinary GET entries remain part of the reviewed
URL. Compiled rules and additive settings disclose the new mandatory workflow.
Native loading status and duplicate activation prevention apply during a
pending form request; existing click cancellation/document fences remain.

POST reload and default URL-history traversal use a native Resend/Cancel alert.
Cancel changes no request count or history cursor. Accept uses a single-use
confirmation bound to tab, document and navigation sequence, then re-runs review.
Reload replaces the current entry. Private request buffers share zeroizing
ownership; each tab retains at most eight POST entries/eight MiB. Expired bodies
require a new form submission and never silently become GET. MCP returns a
confirmation notice without automatically replaying; the reference frontend
reports its unsupported confirmation UI. This is ordinary resubmission UX,
separate from the private human-permission capability boundary.

## Validation corrections and limits

The new range case first failed with an absent default semantic value. Existing
regressions then caught i64 precision loss from unconditional f64 formatting;
valid in-range spellings are now preserved. The final 711 engine cases and
15 public form cases pass with their original assertions.

The form fixture now clears the inherited nonblocking flag on accepted macOS
TCP/Unix sockets; its three-second timeout and assertions remain. Worker cleanup
preserves the original assertion on unwinding. An existing IPC fixture used a
clock timestamp alone and collided during parallel tests; a per-process atomic
counter fixes naming, with all 197 IPC cases passing in parallel. The correction
is entirely inside cfg(test) and does not change the native app's runtime code.
An initial MCP private-core startup missed its original five-second deadline;
the freshly linked local binary was signed/warmed and the unchanged assertion
passed in the final complete 191-case run. UI tests select sheet buttons within
the exact owned window to avoid the separate Touch Bar Cancel element.

Complete HTML type/pattern/length/numeric constraints, validation bubbles and
invalid-field focus, submit/formdata/reset DOM events, dirty/default properties,
image submitters/coordinates, dirname, alternate targets, dialog forms and real
file selection remain pending. Multiple-select submission is covered, while its
native interaction model is pending. HTTPS origin decisions have compiled rule
and real-process tests; the loopback form fixture exercises HTTP transport and
policy denial, not a successful TLS form exchange.

Physical system Zhuyin explicitly skips because the UI Runner lacks Accessibility
permission. Deterministic AppKit composition passes; no TCC/privacy setting was
modified. Actual VoiceOver, Linux/Windows UI, Intel/older macOS, Release signing,
notarization, full Rust workspace execution and coverage were not rerun here.

## Reproduction and final evidence

```sh
frontend/macos/test.sh
export CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-engine -p blueice-net -p blueice-ipc -p blueice-html -p blueice-layout -p blueice-ai-gatekeeper --lib --test native_forms --test native_keyboard --test parsing --test navigation --test form_submission --test gatekeeper_binary --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo build -p blueice-launcher -p blueice-downloads --bins --locked --offline
codesign --force --sign - "$CARGO_TARGET_DIR/debug/blueice-core" "$CARGO_TARGET_DIR/debug/blueice-bluejs-host" "$CARGO_TARGET_DIR/debug/blueice-downloads"
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-mcp-server --tests --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo check --workspace --all-targets --locked --offline
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy -p blueice-engine -p blueice-net -p blueice-ipc -p blueice-html -p blueice-layout -p blueice-ai-gatekeeper -p blueice-mcp-server -p blueice-frontend-reference --all-targets --locked --offline -- -D warnings
"$HOME/.cargo/bin/rustup" run 1.96.0 cargo fmt --all -- --check
```

Host: Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0 (27A266a), Swift 6.4,
Rust 1.96.0. Native tests require the existing unlocked graphical session and
Xcode automation authorization; socket tests require local socket access.

Final native bundle: `frontend/macos/.build/results-20261002-220202.xcresult`.
The 37 submission/range Rust/Swift source/test paths remained unchanged across
the final native run, combined SHA-256 `3aa09c173b06b2731703dff6fb768dae636d17b17aa20482ea3e689b5d869059`.
The supplementary cfg(test) IPC fixture file SHA-256 is
`316ee307f51de00b2a5a71208cd44422ad19d3fa1025032bf4afd79b36c789d2` and is covered by the final Rust run.
Hashing uses sorted relative paths, NUL, raw file bytes, NUL; documentation and
generated output are excluded. Base: `4c1c6e922db02b9109edccb2afa192caba0c4710`.

The inspected native-window screenshot is ignored at
`development/browser_core/phase-22-browser-shell-accessibility/artifacts/macos-form-resubmission.png` (243,528 bytes), SHA-256
`65e7e020798f6192b840cde4b0be89574942da38c7695162e46167a530d93c80`. It shows the reviewed POST receipt and the native
Cancel/Resend sheet with the corrected explanation and confirmation status.
Binary images and result bundles remain outside tracked files; the compact
[metadata record](artifacts/macos-form-submission-results.txt) is tracked.
