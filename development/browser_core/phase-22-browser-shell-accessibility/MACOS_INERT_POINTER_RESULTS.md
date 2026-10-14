# macOS inert and painted pointer results

Accepted: 2026-10-10 (Asia/Taipei), Apple Silicon macOS 26.6.2.
Parent commit `7279729ab36990977c5da32cfb5f15314d893652`. Complete macOS browser delivery remains in progress.

Ordinary inert DOM subtrees are transparent to pointer hit testing, including
text descendants and `inert="false"`. The last painted eligible sibling receives
the hit. Inert labels cannot activate external file controls or listeners; inert
children leave an active label or underlay operable. Paint commands are unchanged.

## Regression evidence

Six new public Rust regressions produced five failures and three passes before
two core production files changed; the same test bytes then passed all eight
selected methods. Active targets establish geometry, and rendered pixels verify
the blue overlapping layer at the pointer coordinate. The initial inert-overlay
case already passed through the old incorrect sibling order; the active overlap
control reproduced that ordering defect. Final public focus passed 61 cases in
three groups. An earlier nonexistent test-target command ran zero tests and is
retained separately.

Actual-window testing then reproduced an inert button being classified as a
native control: mouseDown chose focusPage, and mouseUp sent no correlated native
activation or file-picker hint. An unchanged isolated run, a button-first run and
a window-position helper reproduced it. Temporary routing traces were removed
before final inputs were frozen.

Native classification now excludes controls that are both accessibility-hidden
and unavailable for native focus, preserving enabled aria-hidden controls.
Direct file/select interception and select-list wheel routing check existing
native focus availability. The real core still chooses targets and validates
context and gestures. A new XCTest exercises real core services and AppKit events
for span/button/ancestor/file/select profiles and an enabled aria-hidden control.
The corrected test failed on original native routing and passed after exactly
two Swift production files changed, with its bytes unchanged. Its standalone
key-window guard is explicitly simulated; XCUITest verifies actual OS windows.
Earlier fixture and zero-method compile failures remain in the receipt.

## Full acceptance and UI positioning

The first unfiltered native attempt ran 278 methods: 276 passed, one failed and
the physical Zhuyin method skipped. Its owned third tab-drag window reached
(48, 40) rather than (40, 40); the unchanged method passed in isolation. The second
full attempt ran 274 passes, three positioning failures and the same skip.
Owned-window attachments show x positions 47, 43 and 44 instead of 40, with the
expected y positions and sizes. The underlying synthesis cause is not established.
Both failed runs and their exact scopes and geometry remain in the receipt.

The private positioning helpers now capture a single frame and use an integer
title point with at most three native drags toward the existing target. The
original less-than-three-point assertion, window sizes, and all 122 UI test
method bodies remain unchanged. The three formerly failing complete feature
methods pass in isolation. Expanded focus passes 15 methods: one XCTest and
fourteen actual-window XCUITest methods, zero failures and zero skips. Fresh
unfiltered native acceptance then executes all 278 methods: 156 XCTest passes,
121 XCUITest passes, zero failures and the same one physical Zhuyin skip.
The unfiltered run records 5 corrective
retry steps that reached the unchanged positioning condition after an earlier
drag missed it. Per-method native coordinates are retained in the receipt.
All 273 parent methods remain present. An additional sandbox startup attempt
failed its relocated platform-executable trust check before any method ran;
it is retained separately, and unchanged tests ran in the approved host context.

Formatting, strict all-target Clippy, all-target build and the Rust workspace
passed on original freeze `679025f45c737c80831707c9ac80aa9e63f9b87f668492fbeceb9ef2ce4c4570`. The workspace has 7,401
passes, zero failures and 69 ignored cases across 483 result groups. These Rust
results are reused with their actual commands, dates, logs and original input
manifest. Corrected native gates use `1adf12c38c2ff66d5b2edcb26b90d217840ece048ee46ded08d1bbb7f75714a8`. Both manifests
contain 1,837 inputs and differ only in `UITests/BrowserUITests.swift` positioning
helpers: all other 1,836 inputs, including Rust and Swift production sources,
and all 122 UI test method bodies are unchanged. The receipt explicitly records
this fixture exception; it does not claim the Rust gates ran again on that file.

Four new actual-window methods assert inert label/link listeners, external file
picker opening/cancellation, active labels with five inert child profiles,
painted top-link and transparent inert-layer navigation, retained values,
document identity and exact HTTP destinations. Seven existing native label/file
methods and the three positioning-dependent methods remain in expanded focus.

## Product and cleanup evidence

The first final-audit attempt failed in its own method-hash slicing logic.
The corrected auditor uses the preserved proof's ordinary/private function
declaration boundaries, including separators and adjacent unchanged declarations.
All 122 spans match. Its failed execution and original source hash remain in the
receipt; GUI source bytes and native test results were unchanged.

11 strict signature checks passed; executable and payload
hashes include the actual BlueIce debug GUI dylib. Both Swift app executables
and that GUI payload contain arm64 and x86_64 slices. Runtime and Rust service
acceptance are arm64. The final audit found no owned processes; owned SFTP/update
helpers recorded process/group and private-root cleanup. The unfiltered update
case exercised staged-core lifetime assertions. The existing unrelated launcher
test retains its timestamp and original parent proof. No system security or
accessibility setting was changed. Verification images, recordings and xcresult
bundles remain ignored and untracked.

The [contract](MACOS_INERT_POINTER_CONTRACT.md) and
[machine-readable receipt](artifacts/macos-inert-pointer-results.txt) preserve
log hashes, exact methods, original and corrected manifests, failed attempts,
both unchanged-test red-to-green proofs, geometry and product/process evidence.
This increment covers ordinary DOM ancestry and ordinary paint order. Full CSS
stacking/positioned layout, general pointer-events, flat-tree/modal inertness,
native mouse-movement/leave forwarding and physical IME behavior remain open
alongside the [delivery plan](MACOS_DELIVERY_PLAN.md).
