# macOS durable session restoration results

Accepted on 2026-10-03 on Apple Silicon macOS 26.6.2 (25G83), Xcode 27.0
(27A266a), Swift 6.4 and Rust 1.96.0. The base commit is
`bbb4b3af566ddd62e88eb1e40271ea46203d0423`.

| Gate | Final result |
| --- | --- |
| Full native XCTest / XCUITest | `results-20261003-181600.xcresult`: **149 passed, 0 failed, 1 skipped**; 89 XCTest and 60 UI passes, terminal exit 0 |
| Full Rust workspace | **7,208 passed, 69 ignored**, 0 failures; terminal exit 0 |
| Workspace build, all targets | Passed, terminal exit 0 |
| Workspace Clippy, all targets, `-D warnings` | Passed, terminal exit 0 |
| Workspace formatting | Passed, terminal exit 0 |
| Bundle signatures | All 8 strict verifications passed: parent, private panel app and 6 services |
| Owned native bundle processes after UI | **0**; Rust test processes and other bundle prefixes excluded |
| Frozen source/build inputs | **70 unchanged**, aggregate SHA-256 `c735adcf854d4838c879ed619d96fbbce3e5ed32e528e268a67e65fde2e4602e` |

The physical Zhuyin case reports a skip because the generated UI runner lacks
Accessibility trust. No TCC settings were changed. The full native run reports
47 internal runner QoS warnings and 0 SwiftUI view-update warnings. Parent and
private panel app contain both x86_64 and arm64; execution acceptance is arm64.
Focused 4-model/4-UI and 917 Rust passes overlap the full gates and are not added
to their totals. Full commands, hashes and audit receipts appear in
[macos-session-restore-results.txt](artifacts/macos-session-restore-results.txt).

The implementation adds opt-in Remember, independent automatic Reopen, manual
Restore Last Session and Forget to native Settings and File commands. Bounded
versioned metadata preserves profiles/windows, tab order and selection, group
names/colors/collapse, history cursor/forward branch, zoom and window geometry.
Window placement is constrained to an available screen. Runtime IDs, DOM, form
contents, passwords, selected files, POST bodies and permission grants are absent.

Current GET restoration uses mandatory URL/content review. Live document identity
and current reviewed URL fence history installation. POST records retain only a
URL and expired marker; a fixed native warning replaces the current POST, with
no automatic network request or GET downgrade. Changed-content denial remains
visible and does not import unreviewed HTML.

Native preferences remain separate from trusted assistant/permission child
settings. Saving and Forget flush the preferences store; normal Quit captures
all live windows before teardown. A malformed archive remains intact until
Forget. A failed or over-limit capture keeps the previous archive.

The generated UI runner has its own sandboxed preferences namespace. UI tests
read the application's isolated test-domain plist with the runner's existing
read-only entitlement. Invalid preference type is injected through Foundation's
standard argument domain; unit tests separately verify malformed persisted Data
is retained unchanged. Tests use the ordinary native UI and normal termination.

Screenshots, xcresult and build files remain ignored. No fresh Linux/Windows,
line-coverage, physical IME, VoiceOver, distribution signing or notarization
acceptance is claimed. Fullscreen/miniaturized state and network storage
partitioning remain pending.


The final unedited screenshot is `artifacts/macos-session-restored.png`, SHA-256
`f583cf564bc9ca3e951130a9541a40b064a83f83bde8b903658a55f1a7cd87a6`.
It was inspected and remains ignored. The existing system Local Network prompt
is visible; it was neither accepted, dismissed nor edited out.

Exploratory runs exposed the generated runner's separate preferences namespace,
blank-tab opening's exact `TabOpened` terminal reply, asynchronous preference
removal, and fixture text assertions. The accepted tests read the isolated app
domain, correlate opening replies with canonical ownership, verify persisted
removal and assert the actual fixture content without reducing behavior checks.
Exploratory failures are excluded from acceptance totals.
