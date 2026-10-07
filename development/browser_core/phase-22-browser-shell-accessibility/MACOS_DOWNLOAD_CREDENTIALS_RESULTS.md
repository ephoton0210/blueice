# macOS download credentials validation

Base: `a212f4c2a4dc2ec49075aa473ab976793bbfdfef`.
Native credential-store UI and its private account resolver are implemented and
accepted. See [the contract](MACOS_DOWNLOAD_CREDENTIALS_CONTRACT.md) and
[the machine-readable record](artifacts/macos-download-credentials-results.txt).
The complete macOS browser delivery remains active.

## Delivered behavior

Downloads > Credentials reviews an SFTP/FTPS account using the actual transfer
URL parsers, then saves or removes the selected password or private-key
passphrase in the actual macOS Keychain. Resolution starts no network request
or transfer. Existing credential commands and server verification remain in use.
Native SecureField masking, cleared account/kind/dismissed drafts, keyboard Save,
English/Traditional Chinese labels, normal relaunch, namespace separation,
idempotent deletion, secret-size and stopped/foreign-session refusals were
exercised. No host permission, Keychain ACL or security setting changed.

The private-key file is separate process configuration; this increment manages
its stored passphrase. Native private-key file selection/configuration and
successful authenticated live-server transfers remain separate acceptance work.

## Accepted verification

The complete native run started `2026-10-07T09:21:45.984465+00:00` and finished
`2026-10-07T10:34:58.727079+00:00` in 4392.806 seconds. The authoritative
bundle is `frontend/macos/.build/results-20261007-172210.xcresult`; generated bundles and verification PNGs stay ignored.

| Gate | Actual result |
| --- | --- |
| Complete native suite | 235 passed, zero failed, 1 existing physical Zhuyin skip |
| Native method scope | Exact 236 unique methods: all 231 prior search-baseline methods plus five credential methods; no missing, extra or duplicate methods |
| Final focused native run | 13 passed, zero failed/skipped, 166.730 seconds |
| Related net/downloads/IPC gate | 521 passed, zero failed/ignored, 20 suite groups |
| Complete Rust workspace | 7,314 passed, zero failed, 69 existing ignored cases across 475 suite groups |
| Formatting, strict workspace/all-targets Clippy, all-targets build | Passed with explicit Rust 1.96.0 and one shared Cargo target |
| Native signatures and architectures | Eight strict verifications passed; Swift app and private panel app contain arm64 and x86_64 |
| Source/product/process audit | 1,809 source inputs, eight current executable hashes, zero owned native or pipeline processes |
| Actual screenshots | English and Traditional Chinese reviewed, ignored and hash-matched |

The complete native summary contains 81 internal runtime warnings; their
actual entries and earlier failed attempts remain in the record. A green native
suite does not establish physical IME/VoiceOver or live-server authentication.
The physical test skipped with the actual recorded reason:

> Grant Accessibility to BrowserUITests-Runner.app to permit physical IME key events;
> XCUITest string keys cannot verify Zhuyin hardware mapping.

Rust/static and complete native acceptance share the identical source aggregate
`0321329758d4d2bc55e90865f1ce2d854dfa6290dae4372702522ca820e146a7`. The earlier focused native run differs only in
`backend/downloads/src/server.rs`: strict Clippy required removing the needless
borrow of the unchanged fixed diagnostic passed to `impl Into<String>`.
Byte hashes and reconstruction validate that sole difference. No Swift, wire
shape, tests or other source changed between focused and final acceptance.
All affected Rust tests also ran in the final workspace gate.

Pre-existing processes classified by their actual start times and parentage
were preserved. Exact PIDs, parent commands, scope, signatures, executable hashes,
warning/skip entries and gate durations are retained in the record.

## Development history

- The test-first public resolver regression failed because its API was absent,
  then both implemented public network tests passed.
- An initial socket fixture was denied by the execution sandbox before binding;
  the normal escalated actual-socket rerun passed. This was not an approval-review
  rejection.
- Native attempts 1 and 2 stopped before method execution: the strict Swift gate
  rejected deprecated one-argument `onChange`, then the Swift compiler rejected direct async
  DirectoryEnumerator iteration. Current closures and materialized URL arrays
  corrected those compile errors.
- Native attempt 3 passed 11 methods and failed two fixture expectations: Escape
  closed the entire modal, and the Chinese title exposed native static text
  through value rather than label. The fixture now verifies actual dismissal and
  visible label/value text. Attempt 4 passed 13 methods; attempt 5 added stronger
  foreign-session, incompatible-kind, empty/oversized-secret and draft-clearing
  assertions and passed all 13.
- First Rust pipeline passed 521 related methods, then strict Clippy stopped at
  the needless String borrow. The second pipeline passed formatting, Clippy,
  all-targets build and the complete workspace. No weaker gate or lint exemption
  replaced the failure.

## Remaining delivery work

Native private-key file/configuration UI, authenticated live-server acceptance,
download destination selection, automatic response downloads, vector PDF,
physical printers, full storage partitioning, physical IME/VoiceOver and other
[delivery milestones](MACOS_DELIVERY_PLAN.md) remain open. The earlier observed
family-emoji missing-glyph rendering is unresolved. No new Linux/Windows or Intel
runtime acceptance is claimed; local ad hoc signing is not distribution signing.

Reviewed ignored screenshot `development/browser_core/phase-22-browser-shell-accessibility/artifacts/macos-native-download-credentials-final1.png`:
SHA-256 `e87ddbd479df2faf3f21bc3789af08aaf392d7ce557ba77638f822030f3b6271`.

Reviewed ignored screenshot `development/browser_core/phase-22-browser-shell-accessibility/artifacts/macos-native-download-credentials-zh-final1.png`:
SHA-256 `fea44d64461f2769771bbcee132670068f3ba4ba83ec567c73cea08a351b05e7`.
