# macOS native assistant and translation acceptance — 2026-10-03

Snapshot base: `b53a91111651b9c28d64b0db54bea254354ce6f8`.
Host: Apple Silicon Mac mini, macOS 26.6.2 (25G83), Xcode 27.0
(27A266a), Swift 6.4 and Rust 1.96.0. This increment follows the
[macOS delivery plan](MACOS_DELIVERY_PLAN.md).

## Delivered behavior

The SwiftUI sidebar opens from the native Assistant menu, Command-Shift-A or
toolbar. Summarize and Organize use the Rust core's visible text, excluding
protected field values and hidden content. Organization accepts a bounded
instruction. Results are selectable, verbatim text; HTML and Markdown are not
interpreted as actions or presentation. Closing the sidebar retains a current
result. Each tab owns its result and instruction, including when moved to a
different native window while inference is pending.

The additive `AssistantPage` command carries tab, frame source and document
generation. The core checks all three before reading text or toggling its
translation, and checks existing profile/window ownership first. Summary and
organize replies echo the document, kind and original request ID. The native
controller checks that identity and its local revision before presentation.
Navigation, close, shown-text changes, stop-waiting and deadlines discard late
results. Stop-waiting explicitly allows an already running local task to finish;
it does not promise inference cancellation. Pending operations are bounded and
expire after 65 seconds. The core's existing inference deadlines still apply.

Native document-bound results stay with the request and do not populate the
legacy shared `about:assistant` panel. Existing legacy messages retain their
wire shape and shared-panel behavior. MCP and the reference frontend tolerate
the additive reply without displaying another client's result. Assistant errors
stay inside the sidebar, preserving mandatory browsing-denial status. The
address field may retain the denied draft; the source caption continues to name
the actual committed document.

Translation Apply/Off changes the core-wide target for future navigations in
all windows and profiles. Confirmed acknowledgements save/remove the language
tag in the existing native preference domain. Startup restores and confirms it
before initial navigation. Show translated page toggles an available translation
without refetching, using the same document-bound command. State inspection and
an exact matching acknowledgement precede UI changes; sending a command alone
never reports success. Model settings and permission decisions continue through
the existing launcher-owned private child. The sidebar has no grant authority.

## Verification

Final counts, frozen source hashes, terminal exits, signatures and teardown are
recorded in [macos-assistant-page-results.txt](artifacts/macos-assistant-page-results.txt).

| Gate | Evidence |
| --- | --- |
| Full native XCTest / XCUITest | `results-20261003-161634.xcresult`: **141 passed, 0 failed, 1 skipped** (85 XCTest and 56 UI passes), terminal exit 0 |
| Full Rust workspace | **7,204 passed, 0 failed, 69 ignored**, terminal exit 0 |
| Rust all-target build / strict Clippy / Rustfmt | Passed, each terminal exit 0 |
| Focused engine assistant / translation / context regressions | **26 passed**, terminal exit 0; included in the workspace total |
| Project/plist lint / shell syntax / whitespace / MPL headers | Passed |
| Local strict signatures | App, native child and six services: **8 verified** |
| Native teardown | **0** app, bundled-service, child or UI-runner processes |

The final run used 66 frozen inputs with aggregate SHA-256
`1c66d55a41fb5fe2d48faeddc3e464b4edb60e30fd612f734a1048aaacace224`.
The receipt records each hash; aggregate input is sorted relative path bytes,
NUL, file bytes, NUL. All inputs were unchanged after the accepted run. The
bundle reports **43 internal QoS warnings** and **0 SwiftUI view-update
warnings**. The sole skip is physical Zhuyin because the runner lacks
Accessibility trust. Parent app and private child contain x86_64/arm64 slices;
this acceptance actually ran on arm64.

Six new XCTest cases cover exact result identity/kind, verbatim text, stale
documents, stop/deadline/close, pending ownership transfer, translation
acknowledgements and shown-text invalidation. Five actual-window cases cover:

- Real-service summary and organize, protected/hidden input privacy, literal
  output, canonical source, close/reopen and no extra page fetch.
- Delayed replies across navigation, stop-waiting, and a subsequent valid task.
- Available translation toggling, no refetch, saved language on app relaunch,
  and acknowledged Off.
- Unavailable assistant failure while a mandatory content denial stays visible
  and the committed page/source remains unchanged.
- A pending result following the existing tab to another native window.

The deterministic model is a bounded HTTP server at numeric loopback. Only
inference output is scripted; the app, launcher, assistant, core, gatekeeper,
broker and native windows are real bundled processes. Independent settings and
preferences isolate each UI test. Core regressions use the same profile/window
command nesting as the GUI, cover rejected document/owner mismatches, and
preserve legacy results. Protocol round trips cover all new action shapes.

Exploratory runs found a missing profile-command allowlist entry, SwiftUI
container identifier propagation, a test waiting for an intentionally disabled
pending-task button, and assertions confusing a denied address draft with the
committed page. The final cases preserve the behavior assertions at the actual
native boundary. A core test socket startup race was fixed with a bounded
connection-refused retry. Exploratory failures are excluded from final counts.

## Artifacts and limits

Screenshots, build output and xcresult bundles remain ignored; this record and
the text receipt are tracked. The inspected screenshot is
`artifacts/macos-native-assistant-result.png`, SHA-256
`2551516a1c5f67a5f98052317c3753e72714d53dded3939267ee05404f955351`.
It is retained without editing; the pre-existing system Local Network prompt
remains visible and was neither accepted nor dismissed.

Deterministic inference verifies wiring and GUI behavior, not model quality.
Applying a language affects future navigations and does not translate an already
loaded original page. Candle inference requires its feature-enabled service and
compatible model files. Physical OS IME, VoiceOver, localization, panel explicit
appearance, durable restoration, extension installation and other milestones
remain open. This increment adds no fresh Linux/Windows, line-coverage,
distribution-signing, notarization or physical hardware acceptance.
