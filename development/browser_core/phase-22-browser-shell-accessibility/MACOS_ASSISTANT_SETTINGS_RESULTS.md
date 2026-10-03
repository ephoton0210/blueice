# macOS native assistant settings acceptance — 2026-10-03

Validated snapshot base: `f83c9415288b02758c2d2624e115eafe9e8d9fd3`.
Host: Apple Silicon Mac mini, macOS 26.6.2 (25G83), Xcode 27.0
(27A266a), Swift 6.4 and Rust 1.96.0. This increment follows the
[macOS delivery plan](MACOS_DELIVERY_PLAN.md).

## Delivered behavior

The launcher-owned SwiftUI/AppKit child now offers Permissions and Assistant
Settings tabs. They share the existing bounded, serial anonymous pipe.
Switching tabs cancels both local confirmations and reinspects the chosen panel;
the launcher's existing handler also invalidates an unfinished one-shot document
review on an intervening request. Closing the native window cancels local
confirmation. Reopening inspects settings again and resets the draft.

The assistant editor shows settings actually in force and edits all existing
settings fields: backend, idle timeout, memory ceiling, scheduling niceness,
loopback provider/base URL/model and Candle model/tokenizer/context. AppKit file
choosers select local model paths. Review changes shows complete before/after
values; Confirm and apply sends the fixed reviewed draft over the private pipe.
The draft editor is disabled while confirmation is visible. Local range/schema
checks precede review, and the existing launcher validator checks every mutation.
The UI waits for an exact current-settings acknowledgement before reporting
success. Rejections and inconsistent replies require inspection again.

The AI proposal panel displays its ID, SHA-256 digest, remaining lifetime and
complete differences derived from the typed current/proposed settings. Review
approval and Confirm and apply are separate gestures. Approval names the exact
ID and digest displayed; refresh discards a half-made confirmation. Deny requires
a reply with unchanged current settings and no pending proposal. Inspection,
drafting, cancellation, closing and switching panels never apply settings.
Control and bidi formatting characters in consent values are displayed as
explicit `[U+XXXX]` escapes, preserving distinctions without changing direction
or impersonating native controls.

The main app starts the existing settings service with a default file at
`~/Library/Application Support/BlueIce/assistant-settings.json`. A missing file
means Off and is not created by inspection. Owner startup `--assistant-settings`
selects a file; confirmed edits are validated, atomically saved with existing
private file permissions and applied to the supervisor. The app bundles the
on-demand `blueice-ai-assistant` beside its other five services. The existing
operator protocol can propose and inspect, while decisions remain exclusively
on the launcher's native-child pipes. Owner startup `--control-socket` chooses
that operator rendezvous. Mandatory browsing review and extension permission
decisions retain their separate checks.

## Verification

The final counts, source hashes, terminal exits, signatures and teardown are
recorded in [macos-assistant-settings-results.txt](artifacts/macos-assistant-settings-results.txt).

| Gate | Evidence |
| --- | --- |
| Full native XCTest / XCUITest | `results-20261003-145105.xcresult`: **130 passed, 0 failed, 1 skipped** (79 XCTest and 51 UI passes) |
| Launcher settings service | **12 passed**, terminal exit 0 |
| Trusted-window filtered regressions | **5 passed**, terminal exit 0; overlaps the settings-service cases |
| Rustfmt / project and plist lint / shell syntax / whitespace | Passed |
| Local strict signatures | App, native child and six services: **8 verified** |
| Native teardown | **0** app, bundled-service, child or UI-runner processes |

All verification commands completed with exit code 0. The native build uses
50 frozen inputs with aggregate SHA-256
`b09e62764a19da68f438bbb167a27dfbd087c6baa8588ff6dda0a45dfe302df5`.
The receipt records individual hashes. Aggregate input is sorted relative path
bytes, NUL, file bytes, NUL; all inputs remain unchanged after the accepted run.
The bundle reports **38 internal QoS warnings** and **0 SwiftUI view-update
warnings**. Physical Zhuyin is the sole skip because the runner lacks
Accessibility trust. These results cover the final compiled source snapshot.

Five new XCTest cases cover inspection/cancellation, immutable reviewed edits,
matching acknowledgements, exact proposal ID/digest, refresh invalidation,
denial, invalid ranges/model URLs and escaped hostile consent metadata.
Four actual-window cases operate the bundled services and real private child:

- Draft/review/cancel, invalid range, confirmed save and native app relaunch.
- Rule-blocked AI proposal, rejected ordinary approval route, canceled/refreshed
  confirmation, actual human approval and denial with persisted-state checks.
- Local model endpoint/name editing, remote endpoint rejection, reachable
  confirmation and close/reopen cancellation before an actual settings save.
- Switching between a live one-shot document review and a settings draft;
  neither confirmation survives and the page is fetched only once.

The tests use independent settings files and short operator sockets under the
runner's temporary directory. The runner has test-only server/client networking
entitlements; production binaries gain no new entitlement or approval route.
An initial UI test attempted `ps` to discover a socket and encountered runner
sandbox denial. An explicit test socket outside its accessible temporary tree
also failed. Selecting a short socket inside that tree allowed the real
operator/native-window test to pass. These exploratory failures are excluded
from accepted results; no consent assertion or production check was weakened.

Existing launcher settings-service regressions exercise rejected wrong IDs and
digests, native edit versus agent rules, denial, atomic-save failure, supervisor
reconfiguration and truthful private replies. Rust source is unchanged by this
increment; the full workspace acceptance recorded in
[permission results](MACOS_PERMISSION_RESULTS.md) remains the base evidence.

## Artifacts and limits

Screenshots, native build output and xcresult bundles remain ignored. The inspected, unedited
native screenshot is `artifacts/macos-native-assistant-settings.png`, SHA-256
`064b24aebd606701ac55156be5f6578e9ed378c744e42f2e263e468fe9b7a57b`.
A pre-existing macOS Local Network prompt overlays the capture; it was neither
accepted nor dismissed. No screenshot editing is part of acceptance.

This verifies configuration and human decisions, not model answer quality or
physical hardware transitions. Default services support loopback inference;
Candle inference requires a feature-enabled service build and compatible model
files. Physical OS IME, VoiceOver, panel explicit appearance/localization,
assistant result/translation surfaces, extension installation and general
site/OS permission prompts remain open. The other delivery milestones retain
their separate acceptance, including durable restoration and final accessibility.
