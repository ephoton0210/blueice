# macOS labels for non-rendered controls results

Accepted: 2026-10-09 (Asia/Taipei), on Apple Silicon macOS 26.6.2. Parent commit
`1dfcd7930c96ba12d952e13c863182d68efda2cc`. Complete macOS browser delivery remains in progress.

Visible explicit and implicit labels can now activate enabled associated file,
checkbox, radio and submit controls without a layout fragment or with the HTML
`hidden` attribute. An input whose type is `hidden` remains non-labelable.
Disabled controls, disabled fieldsets, inert control subtrees, canceled clicks,
changed association, replaced documents and interactive descendants retain their guards.

Activation availability is separate from native focus geometry. Hidden controls
stay outside native Tab order and cannot take native focus. A visible tabindex
label retains its own focus, including when a hidden radio changes its group.
Existing visible controls retain their editing and focus behavior.

The existing correlated gesture identifies the actual file control. AppKit
validates its tab/source/document/revision context without requiring an AX node
or dispatching another click. Real native selection retains the existing data
and regular-file limits. Cancellation keeps the selection; reset clears current
selections and invalidates old picker contexts while retained immutable File
values remain readable. Hidden submit defaults use the existing navigation gate.

## Validation

Five new public regressions first produced four actual method failures while
disabled/inert restrictions passed. After the correction all five pass, together
with all 55 keyboard, file-selection and script-file interface regressions.
The complete Rust workspace passed 7,388 cases, zero failures and 69 ignored
cases across 482 result groups. Formatting, strict all-target Clippy and the
all-target build passed.

The original controller failed with `BrokenPipeError` after the Rust commands
had completed successfully. The missing native stage resumed separately on the
same inputs; no Rust gate was repeated. The first recovery launcher failed before
starting any test because it called `setsid` while already a process-group
leader; its corrected launcher retains an isolated group and redirects logs to
files. Both controller attempts remain in the receipt.

A Rust auto-update test fixture left one core descendant in the original test
group. Its exact PID, group, start time and private fixture executable were
verified before SIGTERM cleanup. The group was confirmed gone and unrelated
preexisting processes preserved. This was explicit test-fixture cleanup;
automatic fixture descendant cleanup is not claimed.
The cause of that retained auto-update descendant remains unproved and requires
a separate lifecycle regression; it is not resolved by this label increment.

All 13 focused native methods passed with zero failures or skips. Three new
XCUITest methods exercise real explicit/implicit label clicks and AppKit file
panels, binary bytes and Unicode filenames, cancel/reset and retained File reads;
disabled/inert/canceled/interactive-child rejection; and hidden checkbox, radio
and submit defaults through an actual GET with `check=on`, `choice=first` and
`submit=sent`. Existing native file, label, model and synchronous AX tests pass.

The unfiltered XCTest/XCUITest suite executed exactly 272 methods: 271 passed,
zero failed and the same physical Zhuyin method skipped. All 269 parent methods
were retained. The skip requires Accessibility permission for physical IME key
events and does not verify real Zhuyin mapping.

Every final gate and the focused native/public regressions shares the same
unchanged 1,834 source/build inputs, aggregate SHA-256
`4bdc657f982130f7ceacb5fa6d24c95aeee5ed4014e74a0717b8bf7d90e10407`. Three fresh UI images were visually reviewed and remain
ignored by Git. Ten strict signature checks passed and ten executable hashes
were recorded;
both Swift app executables contain arm64 and x86_64 slices. Runtime acceptance
and Rust service products are arm64. Owned gate/native processes and both
loopback SFTP fixtures were cleaned up; unrelated preexisting processes were
identified by their original PID/start/executable and parent proof.

The [contract](MACOS_NONRENDERED_LABEL_CONTRACT.md) and
[machine-readable receipt](artifacts/macos-nonrendered-label-results.txt) retain
the exact commands, manifests, method sets, Xcode summaries, log hashes, images,
product and cleanup audit, and unsuccessful test-first attempt. That red-to-green
delta changes only two production Rust files; the added tests are unchanged.

The added rendering cases cover `display:none` and the HTML `hidden` attribute.
Complete DOM/event/CSS focus conformance, file streams, FileReader/object URLs,
directory/capture selection, general drag/drop, storage partitioning, remaining
private owner panels, remote interoperability, distribution signing and physical
IME/VoiceOver/printer/display acceptance remain in the
[delivery plan](MACOS_DELIVERY_PLAN.md).
