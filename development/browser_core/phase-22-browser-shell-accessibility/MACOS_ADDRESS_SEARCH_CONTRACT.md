# macOS address-bar search contract

Status: implemented and accepted (2026-10-07).
See [complete acceptance](MACOS_ADDRESS_SEARCH_RESULTS.md).

The address field supports explicit URLs and ordinary search text. Enter/Go
submit one normal core navigation; typing does not contact any provider. Bare
DNS names, localhost, IPv4/IPv6 and optional ports retain URL navigation. Explicit
schemes, including unsupported schemes, are passed to core rather than leaking
an intended URL to a search provider. A leading question mark forces search.
UTF-8 queries use exact percent encoding, preserving CJK, emoji, literal plus,
ampersand, percent and hash as one query value. Bound/control checks refuse
invalid input before navigation; they do not silently fall back to another
provider.

Native Settings offers DuckDuckGo (default), Google, Bing and a custom HTTP(S)
endpoint/query field. All windows share the persisted choice; new windows,
normal relaunch and service recovery use that preference. A custom endpoint may
retain fixed query fields, but submitted query occurrences are replaced by one
properly encoded value. Credentials/fragments and malformed provider settings
refuse search. Ordinary URLs still navigate with invalid search settings.
Unknown stored providers require an explicit selection instead of silently
sending search text elsewhere.

Historical six-method XCTest passed with zero failures in an arm64 temporary
bundle compiled directly from the prior app and test sources, without launching a
browser window or service stack. The Boolean-version fixture verifies the real
CFBoolean storage type after explicitly replacing the prior valid preference.
App/XCTest and UI-source Swift 5 warnings-as-errors compiler checks passed, and a
pure resolver CLI passed 25 cases. Four new failing assertions reproduced opaque
`mailto:`, FTP and custom-scheme misclassification; the corrected bare-host
heuristic rejects user-info and requires a DNS, localhost or IPv6 host. The
latest six-method XCTest run passed after that correction, including DNS ports.
The integrated focused native suite subsequently passed all six XCTest and
five XCUITest methods; complete native regression also passed.
The history regression changes the provider before Back/Forward, requires the
original confirmed URL and page, and permits either a cached response or one
refetch of that same URL. Changing the provider alone must issue no request.
Custom search Save is the native default action: Return saves the draft or
shows its validation error, using the same action as a pointer click. The
localized keyboard regression submits an invalid draft with Return and then
checks a valid pointer save. These view changes passed current native focused
execution; the earlier headless unit receipt precedes this keyboard addition.

Acceptance requires resolver regressions, real-core human Return/Go and history,
no request while typing, provider Settings persistence/window ownership,
malformed input/provider refusal, localization/keyboard interaction, normal
Gatekeeper denial and no AI bypass. Complete native and relevant Rust/static
gates must pass with source/product/process evidence before milestone commit.

The search-only source delta was reconciled onto accepted quarantine commit
`e79cbdc08`. The same main-worktree Cargo cache and a serialized native test
pipeline are used. The accepted quarantine Rust fixtures and evidence remain
intact; no isolated-worktree acceptance document replaces them.

Provider references: [DuckDuckGo URL parameters](https://duckduckgo.com/duckduckgo-help-pages/settings/params),
[Chrome search-engine settings](https://support.google.com/chrome/answer/16739353).

Focused screenshot review observed missing-glyph boxes for the family emoji in
the current core renderer. Query transport and accessible text preserve its exact
Unicode value. This contract does not accept emoji glyph rendering.
