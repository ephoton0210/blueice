# macOS native extension permission acceptance — 2026-10-03

Validated snapshot base: `222d7f78e6a6a3d769a3502d10e4c2d0da911e5a`.
Host: Apple Silicon Mac mini, macOS 26.6.2 (25G83), Xcode 27.0,
Swift 6.4 and Rust 1.96.0. This is the installed-extension permission
increment of the [macOS delivery plan](MACOS_DELIVERY_PLAN.md), not final
acceptance of the whole browser or every trusted panel.

## Delivered behavior

The bundled launcher resolves a fixed nested `BlueIcePanels.app` and starts
that exact native child with the existing anonymous stdin/stdout permission
pipes. No arbitrary frontend path or new public grant protocol was added.
Unpackaged launchers retain their ordinary sibling frontend. The SwiftUI/AppKit
child owns only permission chrome; it creates no page, DOM or renderer.
Its browser connection sends only Hello/ListTabs to inspect the same core's tabs.

The main toolbar activates only the matching bundle in its owned launcher's
process group. The child initially stays hidden while inspecting permission
state for the launcher's existing readiness barrier. Closing its window hides
it and cancels a decision awaiting confirmation. Losing the private child ends
the broker; the main app reports connection loss and disables browser actions.

Optional permission rows display package name/version/digest, capability,
origin scope and actual grant state. Allow/Revoke first opens a local confirmation,
then submits the exact package/core-generation decision. State changes only
after a matching core reply. Rejection or an inconsistent acknowledgement
requires another inspection; sending a request is not treated as success.

One-time DOM access separately reviews the live HTTP(S) tab/document from the
private core-parent boundary, displays its URL, and requires a distinct Allow
one read action. Core rejects replaced documents. A confirmation consumes the
review, and no bearer ticket appears in the native reply. Cancel never sends
an Arm request. Ordinary browser, extension and MCP paths gain no grant route;
mandatory navigation/content review remains in force.

There is no installed package by default. The explicit main-app startup option
`--extension-manifest` forwards an owner-selected package through the existing
launcher/core validator. The separately bundled extension host executes it.
This increment provides no page-driven install or manifest-path replacement.

A real UI regression reproduced diagnostic-mode permission-window failure
overwriting `Navigation blocked: the gatekeeper is unreachable`. The toolbar
now presents a separate Permissions unavailable alert and preserves that denial.

## Verification

All commands completed with exit code 0. The dated machine-readable evidence is
[macos-permission-results.txt](artifacts/macos-permission-results.txt).

| Gate | Evidence |
| --- | --- |
| Full Rust workspace | **7,201 passed, 0 failed, 69 ignored**, 468 result groups including doc tests |
| Workspace all-targets build | Passed using the existing `.build/core-target` |
| Clippy workspace/all-targets, `-D warnings` | Passed |
| Rustfmt, project/plist lint, whitespace check | Passed |
| Full native bundle | `results-20261003-133331.xcresult`: **119 passed, 0 failed, 1 skipped** |
| Final scoped native bundle | `results-20261003-140731.xcresult`: **15 passed, 0 failed, 0 skipped** |
| Unique native cases | **121 passed, 1 skipped**; reruns are not added twice |
| Development signatures | App, nested permission app and five services pass `codesign --verify --strict` |
| Native teardown | No app, bundled-service, permission-child or UI-runner processes remain |

The full native build used 46 frozen inputs with aggregate SHA-256
`fca8404ee11f90275a638b529ebf43457d0862049332e79fa1a203914e16c93e`.
The final 46-input aggregate is
`992f6e680be1c57846db568f271ad7eb1335cc453e7f484578664301cdc93ee4`.
The only later input changes are BrowserModel, BrowserView and BrowserUITests:
two additional UI cases and the independent permission-failure alert.
Rust inputs remained unchanged throughout workspace validation.
The artifact records individual hashes; aggregate input is sorted path bytes,
NUL, file bytes, NUL. The full bundle does not claim to test those later changes.

The final scoped run covers all six permission-model tests and six real-window
permission cases, plus both native file-picker cases and the actual Print/PDF
save case. It checks cancel/no implicit grant, confirmed Allow/Revoke and refresh,
empty-state close/reopen, stale-document rejection, separate one-shot confirmation,
an approximately 8 KiB URL with reachable confirmation and no refetch, child-loss
fail-closed behavior, and preservation of a mandatory navigation denial.
Model tests also reject stale/inconsistent acknowledgements and malformed frames.

The Mac-specific launcher path regression passes. Existing installed-Wasm owner
boundary tests pass **3 cases**, with their **3 manual/GTK cases ignored**;
they exercise private optional grants/revocation and single-use DOM access.
The workspace also includes public IPC/extension tests that cannot self-grant
optional or ephemeral declarations. Native fixtures confirm actual core grant
state; they do not claim their no-op Wasm performed every granted operation.

Earlier failed UI bundles were exploratory: StaticText queries were corrected
to its exact `value`, the empty-state check uses committed navigation, and
XCTest's termination/reaping of a launcher-owned child was replaced with an
unambiguously identified PID kill and observed exit. The separate denial
regression was red before the alert fix. A default-sandbox Rust attempt failed
to create local sockets; the authorized local-service run then passed.
These failures are not counted as accepted results, and no assertion,
test threshold or production permission check was weakened.

## Artifacts and limits

The inspected, unedited native permission screenshot is copied to ignored
`artifacts/macos-native-permissions.png`, SHA-256
`8b42ae8015dea6134ed2f4f36555d7c100dbee9071f01b5c02d2d0585c71a7b0`.
It comes from the final Grant/Cancel/Revoke case. A pre-existing macOS local-network
permission prompt overlays the image; it was neither accepted nor dismissed.
Screenshots, compiled output and xcresult bundles remain untracked.

The full bundle reports 35 internal QoS warnings; the final scoped bundle
reports 7. Both report zero SwiftUI view-update warnings. Physical Zhuyin remains
skipped because the runner lacks Accessibility trust. This acceptance does not
establish physical IME, VoiceOver, system-permission choice or printer/monitor
handoff. App and permission binaries contain arm64/x86_64 slices; services and
actual runtime acceptance are arm64. Signing is local ad hoc, not distribution
signing/notarization. Fresh workspace coverage, Linux and Windows acceptance
were not measured here. Older/Rust processes outside this native bundle were
excluded from the native teardown audit.

Assistant results/settings/proposals, extension installation UI, generic
site/OS permission prompts, permission-panel explicit appearance/localization
integration and the other delivery milestones remain open. Existing mandatory
policy and same-core boundaries are retained; this increment does not complete
durable restoration, full profile storage partitioning or final accessibility.
