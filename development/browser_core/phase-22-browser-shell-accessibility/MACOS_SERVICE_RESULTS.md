# macOS supervised navigation validation

Validated on 2026-10-02, Apple Silicon macOS 26.6.2, Xcode 27.0 (27A266a),
Swift compiler 6.4 and Rust 1.96.0. This extends the
[original private-pipe shell](MACOS_RESULTS.md) with the bundled production
launcher and gatekeeper. The default app uses their normal URL/content review
rules and persistent owner settings; no review service or renderer is simulated.

## Results

| Check | Result |
| --- | --- |
| `frontend/macos/test.sh`: service/app build, tests and attachment export | Passed, exit 0 |
| Swift XCTest protocol/frame/process/service cases | 13 passed, 0 failed |
| Actual native-window XCUITest | 10 passed, 0 failed |
| Rust launcher argument tests | 24 passed, 0 failed |
| Rust real-service owner lifetime cases | 3 passed, 0 failed |
| Native GUI terminated by SIGTERM and SIGKILL | Both passed; no remaining services or frame/socket files |
| `cargo clippy -p blueice-launcher --all-targets --locked -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| App/bundled service signature verification, project/plist/shell parsing | Passed |
| Git whitespace and screenshot ignore rules | Passed |
| Remaining private runtime directories after verification | 0 |

The Swift integration tests exercise a real loopback HTTP origin and verify
reviewed green RGBA pixels from the core; compiled malicious-host denial;
hidden prompt-injection content denial; unchanged committed URL, frame and
history after rejection; unavailable review before any HTTP fetch; missing
gatekeeper preflight; duplicate startup without losing the live session;
startup cancellation; repeated shutdown; and bounded process-group cleanup
with a TERM-resistant owner and descendant. The original protocol/frame and
explicit private-pipe regression cases remain included.

The ten XCUITest cases retain the original shell coverage and add reviewed HTTP
pixels, back/forward to an external page, committed-URL reload, content denial
and recovery, and missing-launcher failure with disabled controls. Only the
loopback HTTP origin is a fixture. The runner has a test-only network-server
entitlement; the application still uses the real core and gatekeeper.

The Rust lifetime cases verify EOF before the first browser connects, EOF with
an active browser and pending page broadcasts, service/frame/socket cleanup,
and preservation of the existing shared-broker lifetime without the new opt-in.
The shutdown monitor correlates its Hello by request ID because pending broker
broadcasts can arrive first. Separate native application termination checks
confirmed that both SIGTERM and SIGKILL close the inherited lifetime pipe and
reap the full stack. An abrupt GUI exit can leave an empty runtime parent;
the termination check removed only its own empty parent after verification.
Normal frontend shutdown removes its complete runtime directory.

## Reproduction

```sh
frontend/macos/test.sh
CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target" \
    CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$PWD/frontend/macos/TestSupport/run-signed-test.sh" \
    BLUEICE_TEST_LAUNCHER_EXE="$PWD/frontend/macos/.build/Build/Products/Debug/BlueIce.app/Contents/MacOS/blueice-launcher" \
    "$HOME/.cargo/bin/rustup" run 1.96.0 cargo test \
    -p blueice-launcher --bin blueice-launcher --test owner_lifetime --locked
CARGO_TARGET_DIR="$PWD/frontend/macos/.build/core-target" \
    "$HOME/.cargo/bin/rustup" run 1.96.0 cargo clippy \
    -p blueice-launcher --all-targets --locked -- -D warnings
```

The backend command validates the actual signed native service bundle.
The Cargo runner signs generated test executables for local macOS execution;
without the binary override, the lifetime tests use isolated copies of the
Cargo-built sibling services. No system signing or privacy settings are changed.

The complete native workflow produced
`frontend/macos/.build/results-20261002-142038.xcresult` and exported attachments
into the adjacent `results-20261002-142038-attachments` directory. Its reviewed
HTTP screenshot was copied unchanged to
[`artifacts/macos-reviewed-http.png`](artifacts/macos-reviewed-http.png).
Screenshots, result bundles and build products remain ignored by Git.
The compact record is
[`artifacts/macos-service-results.txt`](artifacts/macos-service-results.txt).

The 21 Swift, shell, entitlement, Xcode/scheme and affected Rust source files
have combined SHA-256
`b876c90506443937363cfa6f22a65a18921089ea2a4b274937e6962dca4706bb`.
The hash uses sorted repository-relative paths, a NUL, raw bytes and another
NUL; documentation and build output are excluded.

| Signed bundled service | SHA-256 |
| --- | --- |
| `blueice-core` | `bf693ec1f527623f05cb18977429cbe8b67ec10cdc7d339f4d8c6cb7cecc3c7c` |
| `blueice-launcher` | `6797a882a017a82abf08f2fae792426b042dc9ae5506032d969c33734350b12a` |
| `blueice-ai-gatekeeper` | `1b5e0de8b1a1df2fb008aa86dc1fc18394285aac47321472eec85238d60d0529` |

## Scope

HTTP(S) navigation uses the existing core transport/review implementation;
the network UI tests use deterministic loopback HTTP. Public HTTPS endpoints,
configured local-model review, Intel hardware, older macOS versions and Release
distribution signing/notarization were not validated in this run. The full
Rust workspace suite and workspace coverage were not rerun.

Existing shared-launcher attachment, assistant/trusted permission panels, full
IME/document editing, clipboard, groups/multiple windows, downloads/printing,
localization and DOM-derived NSAccessibility/VoiceOver remain Phase 22 work.
This validation establishes native shell and supervised navigation behavior,
not complete page accessibility or complete browser-shell delivery.
