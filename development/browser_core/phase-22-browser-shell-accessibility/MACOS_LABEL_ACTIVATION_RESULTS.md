# macOS native label activation results

Accepted: 2026-10-09 (Asia/Taipei), on Apple Silicon macOS 26.6.2. Parent commit
`d42c23a33422cf8fd64b8c801cc2ed4ac7e98861`. This milestone implements HTML label
activation through the existing native controls and file picker. Full macOS
browser delivery remains in progress.

Explicit labels resolve the first matching ID only when it is labelable. Implicit
labels use the first labelable descendant, skipping hidden inputs. Interactive
descendants retain their own action. Original and associated-control click events
run in order before the existing control default. Cancellation, disabled targets,
changed association and replaced documents suppress stale activation.
Forwarding currently requires an enabled control with a layout fragment and no
`hidden` or `inert` restriction on the control or its ancestors. Labels targeting
file controls without a layout fragment remain pending.

A real macOS document click uses a correlated native gesture. Core returns the
actual file control's existing context, and the AppKit picker validates it again
without generating another click. An independent nonzero random gesture identifier
is echoed because launcher broadcasts connected clients' replies while preserving
their supplied request IDs. Unsolicited, mismatched and stale hints do not present
a picker. Native panel selection, cancellation and selected-byte checks retain
the accepted file API behavior.

## Validation

The complete Rust workspace passed 7,383 cases with zero failures and 69 ignored
tests across 482 result groups. Formatting, strict all-target Clippy and all-target
build passed. Eleven new public Rust regressions cover label association, exact
click targets/order, cancellation at either listener, document replacement,
disabled fieldsets, invalid ownership/points/zero gestures, file hints, wire
roundtrips and MCP handling of unrelated completion replies.

The unfiltered XCTest/XCUITest suite executed exactly 269 methods: 268 passed,
zero failed and one skipped. The sole physical Zhuyin skip and all 264 parent
methods were retained, with five new methods. All ten focused native methods
and all 49 focused public protocol cases passed without skips.

Three added UI methods click actual rendered labels, exercise the real AppKit
file picker with exact binary bytes and ordered events, cancel while preserving
the selected file, focus and edit text, toggle checkbox/radio state, activate an
implicit button after a hidden input, and reject canceled, disabled or changed
associations and interactive descendants. Two added native model methods verify
gesture/context wire handling and that only an owned pending gesture can deliver
a file hint. Existing file-selection and cancel/reset tests also passed.

The final full and focused native gates share 1,834 source/build inputs,
aggregate SHA-256
`6db888fb87e47b4a7063f9f4c76c17f6f1b997ef3d9620998dcb444ac747262f`.
The completed full Rust gates used the same production and build inputs before
one existing Swift test synchronization correction, aggregate SHA-256
`a79d236b55abf1a28e10f09583fa0574d66951ba13cf0667f64bc4736ff159e6`.
The recorded post-Rust delta is only `frontend/macos/Tests/NativeEditingTests.swift`;
all Rust sources, production Swift and build inputs are unchanged. The complete
7,383-case Rust acceptance is preserved without claiming a new workspace run.
Three final focused UI images were visually reviewed and remain ignored by Git.
Ten strict signature checks and ten executable checksums passed. Both Swift app
executables contain arm64 and x86_64 slices; runtime acceptance and Rust service
products are arm64. Owned native/pipeline processes and every loopback SFTP
fixture were cleaned up; the preexisting unrelated test processes were preserved.

## Evidence and remaining work

The [contract](MACOS_LABEL_ACTIVATION_CONTRACT.md) and
[machine-readable receipt](artifacts/macos-label-activation-results.txt) retain
the exact manifests, native method sets, Rust counts, result summaries, log hashes,
product signatures, cleanup, reviewed images and unsuccessful attempts.

The first UI selector incorrectly assumed labels had separate actionable AX
nodes; real label coordinates then reproduced the missing picker before the fix.
The first Rust regressions reproduced absent forwarded clicks and incorrect
implicit/explicit associations. One native fixture initially used unsupported
`setAttribute`; it now changes implicit association by removing the first control
subtree while keeping the next control. An incorrectly named XCTest target
selected zero methods and was corrected. The first full Clippy run exposed missing
completion-reply match arms in the reference frontend and MCP; both were fixed
and a public MCP regression was added. A later owned waiter was interrupted before
full gates to add the independent gesture identifier; its focused run completed.
The first complete native run executed all 269 methods with one failed
return-to-tab synchronous AX selection read and the unchanged physical Zhuyin
skip. An exact single-method reproduction on unchanged source passed, so the
original cause remains unproven. Review found a concrete synchronization gap:
selecting a retained tab publishes a cached frame before scheduling its viewport
update. The fixture now waits for a strictly newer frame before the synchronous
AX read; production context fences are unchanged. The corrected method and all
nine label/file focused methods then passed, followed by the complete native gate.
These attempts remain recorded alongside final acceptance. Xcode runtime warnings
remain in the result summaries.

This increment covers built-in labels and the existing native defaults. It does
not establish full HTML/DOM/event conformance or add form-associated custom
elements. Labels targeting controls without layout fragments remain pending.
File streams, FileReader/object URLs, directory/capture uploads, general
page/OS drag-and-drop, remaining BlueTS host type support, storage partitioning,
private owner panels, remote interoperability, distribution signing and physical
IME/VoiceOver/printer/display acceptance remain in the delivery plan.
