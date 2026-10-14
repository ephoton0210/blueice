# macOS interface localization

This milestone adds English, Traditional Chinese and Follow macOS choices to
the native Settings scene. The owner preference is shared by existing browser
windows and the launcher's private permissions/assistant child. Complete native,
Rust and source/product acceptance are recorded in
[the dated results](MACOS_LOCALIZATION_RESULTS.md).

The native shell owns localized toolbar labels, help and accessibility labels,
menus, find controls, tab/group/profile editors, download controls, appearance
and session settings, assistant tools and native confirmation surfaces. AppKit
context menus, file-picker labels, heading descriptions and the custom button
rotor use the same bundled string tables. The parent app, private panel and
XCTest bundle each contain their own English/Traditional Chinese resources.

SwiftUI observes the shared language setting without replacing view or model
identity. Switching language preserves the live core page, tab/window ownership,
editor contents, selection, clipboard ordering and assistant/permission state.
Private-panel refresh uses an owner-scoped notification to read the same
preference; it carries no settings proposal, grant or decision. The existing
visible confirmation and private decision pipe remain the authority boundary.

The language choice is stored as `browser.interfaceLanguage` in the existing
preference domain. `--preferences-domain` selects the owner test/profile store;
`--interface-language system|en|zh-Hant` supplies an explicit startup choice.
The private child inherits only the domain, then reads the same saved preference.
Invalid stored choices fall back to Follow macOS. Language paths are restricted
to bundled `en` and `zh-Hant` resources; unavailable keys retain their English key.

The app sets its own `AppleLanguages` preference before building native menus.
This never writes `NSGlobalDomain`. Native browser controls update immediately;
standard macOS menus and system dialogs use the startup language and update
after restarting BlueIce. Follow macOS resolves the global preferred-language
list independently of the app's previous startup override.

Translated templates keep typed format arguments separate from text. Numeric
metadata keeps its existing formatting. URLs, file paths/names, authored profile
and group names, permission capability tokens, reviewed setting values, page
text and model results retain their original values. Interface language is
independent of `browser.translationLanguage` and never translates the core DOM.

Acceptance requires matching resource keys and format arguments, supported
regional language resolution, invalid/unknown-input checks, preference
reconstruction, actual-window Chinese chrome and menus, live switching across
multiple windows/private panels, unchanged page/editor state, persisted language
after normal close/relaunch, existing native UI regressions, the complete native
suite and Rust/static/source/product/signature/process gates. Screenshots and
Xcode build/results remain ignored; dated text receipts record all attempts.
