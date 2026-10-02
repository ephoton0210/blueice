# macOS page accessibility validation

Validated on 2026-10-02, Apple Silicon macOS 26.6.2, Xcode 27.0 (27A266a),
Swift compiler 6.4 and Rust 1.96.0. This extends the
[supervised native shell](MACOS_SERVICE_RESULTS.md) with an initial page
NSAccessibility adapter. The app uses the real owned launcher, core and
compiled gatekeeper; only the loopback HTTP origin supplies fixture HTML.

## Results

| Check | Result |
| --- | --- |
| `frontend/macos/test.sh`: service/app build, tests and screenshot export | Passed, exit 0 |
| Swift XCTest, including 14 page accessibility cases and 13 existing cases | 27 passed, 0 failed |
| Actual native-window XCUITest, including 2 new page accessibility cases | 12 passed, 0 failed |
| Rust engine semantic snapshot tests | 27 passed, 0 failed |
| Rust IPC library tests | 195 passed, 0 failed |
| Clippy for `blueice-ipc` and `blueice-engine`, all targets, `-D warnings` | Passed |
| `cargo fmt --all -- --check` and Git whitespace checks | Passed |
| App/bundled service strict signature verification and Xcode project parsing | Passed |
| Screenshot/result-bundle ignore rules | Passed; no tracked validation PNGs |
| Remaining private runtime directories after tests | 0 |

The Swift boundary tests verify Rust role/envelope decoding; exact tab,
frame-directory source, frame generation and URL matching; duplicate IDs,
missing parents, broken child links, cycles and negative dimensions; document
scroll and Retina scaling; native hierarchy/roles and screen rectangles;
flipped-view screen conversion and hit testing; disabled/offscreen action
rejection; protected values and unsupported editing; same-URL reload and tab
invalidation; and native object identity across a suspended frame refresh.
Malformed representation decoding produces a bridge-unavailable message
without throwing a pixel-transport decoding error.

The real-service XCTest invokes the native element's accessibility focus and
press methods, observes focused core state and an ordinary `InsertText`
commit, verifies password metadata/value redaction, rejects an action from a
previous same-URL document, and follows a local link through normal reviewed
navigation. This calls the native adapter directly; it does not run VoiceOver.

XCUITest reads the OS-visible semantic tree of the real app: heading and
paragraph names, link, image, checkbox, disabled text input and secure text
input. It verifies absence of display-none content and password values from
the accessibility dump, types into a real core text input, preserves its value
across independent tabs, follows a link and confirms removal of old-document
fields. The complete run retains all existing shell, navigation, pixel,
resize, startup-failure and owned-service lifecycle tests.

The Rust regression test confirms that ordinary native-input capability comes
from the same core predicate used by text input, disabled and textarea controls
do not advertise this capability, and case-insensitive password types carry
protected metadata without a value. The IPC regression decodes older states
without the two additive fields, defaulting both capabilities to false.
The first sandboxed IPC run could not create local sockets; the complete rerun
with local socket access passed all 195 cases.

## Implementation and limits

The viewport owns virtual
[NSAccessibilityElement](https://developer.apple.com/documentation/appkit/nsaccessibilityelement-swift.class)
objects built from the core's existing flat semantic tree. It exposes names,
roles, parent/child links, values, state and screen bounds, plus native focus
and hit testing. macOS 26 and newer use the heading role; older systems fall
back to static text with a heading-level role description.

Each accepted frame requests a representation with at most one outstanding
request per tab. A mismatched reply retries only after its requested document
or frame was superseded; unrelated core errors preserve the pending request.
Semantic actions suspend until the representation matches
the committed page and current frame. Native objects survive ordinary frame
refreshes; document epochs distinguish same-URL reloads, and document/tab
changes invalidate old objects. Invalid or oversized semantic trees disable
the bridge while frame display remains available.

Press/focus returns through the ordinary coordinate click/hit-test/default-
action pipeline. Committed text uses the existing native input path. The
adapter never uses `ActOn::SetValue` or a shell-owned DOM. Checkbox, slider and
select state is currently read-only. The core reports whether an actual native
text-input action is supported; the shell does not infer it from the TextBox
role. Password values are redacted again by the native element even if a
payload contains one.

Interactive VoiceOver speech/navigation was not tested. The core's current
accessible-name and role algorithm remains a limited HTML/ARIA subset. Text
ranges, caret/selection geometry, writable AXValue, live-region announcements,
rotor search, IME composition, password/textarea editing and complete
cross-platform accessibility remain open. Older macOS versions, Intel hardware,
Release distribution signing/notarization, Linux/Windows GUI bridges and the
full Rust workspace/coverage gate were not validated in this slice.

## Reproduction and evidence

```sh
frontend/macos/test.sh
CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target" \
    CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh" \
    "$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
    -p blueice-engine --lib ai_snapshot::tests --locked
CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target" \
    CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh" \
    "$HOME/.cargo/bin/rustup" run 1.96.0 cargo test -p blueice-ipc --lib --locked
CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target" \
    "$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy \
    -p blueice-ipc -p blueice-engine --all-targets --locked -- -D warnings
```

Native tests require an unlocked graphical session with existing Xcode UI
automation authorization. IPC tests need permission to create loopback TCP and
Unix sockets. No system privacy or signing settings were changed.

The complete native run produced
`frontend/macos/.build/results-20261002-151329.xcresult` and the adjacent
`results-20261002-151329-attachments` directory. The page screenshot was
visually inspected and copied unchanged to
[`artifacts/macos-page-accessibility.png`](artifacts/macos-page-accessibility.png)
(511,933 bytes; SHA-256
`d355ef1f24b622cab31eb97496d54e13dae484f68184a9b5d5b3eb39fdf95c78`).
Screenshots, native build output and result bundles remain ignored by Git.
The compact source/build/result record is
[`artifacts/macos-accessibility-results.txt`](artifacts/macos-accessibility-results.txt).

The 24 Swift, shell, entitlement, Xcode/scheme and affected Rust source files
have combined SHA-256
`36ccbf3a6df1017be8c8f1d79a9b670d533683beb3b60744834f08247ac05610`.
The record lists every file. Hashing uses sorted repository-relative paths,
a NUL, raw bytes and another NUL; documentation and output are excluded.
The worktree was based on commit
`32fa07f6b38c31790ef238e7450d00a362cd4208`.

| Signed bundled service | SHA-256 |
| --- | --- |
| `blueice-core` | `539f736cb03c5b42bc07695031b2ab2135a0e2c74de518bce96fdf707f2069a7` |
| `blueice-launcher` | `09d82af4b9366026b102b8e36f6832c0dbf1694eb0f574f9ed64a5a19bf84306` |
| `blueice-ai-gatekeeper` | `8a3283c85bc0a583c926945c12530af7e0eecd37d569fcaff5a2b8ec4e83d0e5` |
