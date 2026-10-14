# macOS download quarantine contract

Status: implemented and accepted (2026-10-07).
The [validation record](MACOS_DOWNLOAD_QUARANTINE_RESULTS.md) documents complete
Rust/native acceptance and the source/product/signature/process audit.

Every download completed by the macOS transfer engine receives system quarantine
and Finder source metadata on its retained file descriptor before the finished
name is published. This applies to empty, segmented and sequential responses,
redirects, resume and explicit overwrite, whether started by the native panel,
link menu or another client of the same manager. Existing Gatekeeper URL and
file-type review remains required; quarantine does not approve a transfer.

Apple's public CoreFoundation quarantine API generates the attribute on a
private, owner-only temporary file. The exact system attribute is copied to the
owned download descriptor with public extended-attribute syscalls and read back.
The source is the final response URL, with user-info, query and fragment removed.
Finder receives a binary property-list source array. No invented quarantine
flags or private Apple API are used. Metadata errors refuse publication and
completion; a pre-existing destination and partial bytes remain available.
Normal empty responses now use the same atomic publication as nonempty files.

The native download list reports “Origin recorded for macOS” only for its current
completed record, checked regular file within the configured root and actual
nonempty quarantine properties. It does not infer protection from remote state
or add a misleading badge to old local files. Open delegates to the normal macOS
file handler; Finder actions and history removal preserve system metadata.
Already completed downloads from older releases are not silently modified.

Required evidence: a RED public transfer regression, completion-callback and
empty/redirect coverage, source-token redaction, unsupported attribute failure
and preserved destination/partial bytes, real bundled-service XCTest, and real
native-window download/status/relaunch/history/Open/Finder XCUITest. Full native,
workspace Rust, strict Clippy, formatting, all-targets build, signatures,
source/product consistency and owned-process cleanup must pass before commit.
The existing physical Zhuyin skip, physical VoiceOver/IME, physical printer,
Intel runtime and distribution signing remain separate acceptance requirements.

API references: [Apple quarantine properties](https://developer.apple.com/documentation/foundation/urlresourcekey/quarantinepropertieskey)
and the installed SDK's CFURL.h and LSQuarantine.h, read on 2026-10-07.
