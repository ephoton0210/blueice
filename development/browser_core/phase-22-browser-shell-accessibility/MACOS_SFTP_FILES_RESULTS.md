# macOS native SFTP files validation

Base: `1833e98065b94e63a64263b235b7d7d0ca07e662`. Native file configuration and owned loopback
SFTP encrypted-key authentication are implemented and accepted. The complete
macOS browser goal remains active. See the
[contract](MACOS_SFTP_FILES_CONTRACT.md) and
[machine-readable record](artifacts/macos-sftp-files-results.txt).

## Delivered behavior

Downloads > SFTP files uses actual AppKit NSOpenPanel controls to choose local
known-hosts and private-key files. Choices remain drafts until explicit Apply.
Versioned preferences retain paths only. Cancelling, dismissing, malformed
preferences, missing files and invalid file kinds have regression coverage.
Apply and Restore checkpoint the owned download manager, preserve the browser
core, invalidate prior reviewed credentials and leave unfinished transfers
paused through normal browser relaunch until explicit Resume.

The encrypted-key UI case selects a generated encrypted RSA key and strict
known-hosts file, applies them, reviews its account again, stores the key
passphrase through the native Keychain surface and downloads exact payload
bytes using BlueIce's real SFTP transport. The native Remove action deletes
only that owned credential, followed by a metadata-only absence check.
No key contents or passphrases become preferences, page fields or AI tools.

## Actual acceptance

The complete native pipeline ran from `2026-10-07T11:19:37.212986+00:00` to `2026-10-07T12:32:30.927036+00:00` in
4373.707 seconds, using the normal unfiltered `frontend/macos/test.sh`.

| Gate | Actual result |
| --- | --- |
| Complete native suite | 245 passed, zero failed, one existing physical Zhuyin skip |
| Method scope | Exact 246 unique methods: prior credential baseline 236 plus ten new methods; no missing, extra or duplicate cases |
| Focused native suite | 25 passed, zero failed/skipped, 630.135 seconds |
| Source consistency | 1,816 inputs with identical focused/full aggregate `82dc092540ece6d3b58db9ef0423d243749a5ff33ff86ee452528b8078943839` |
| Signatures and app architectures | Eight strict verifications passed; two Swift apps include arm64 and x86_64 |
| Product and process audit | Eight current executable hashes; zero owned native, pipeline or fixture processes |
| Fixture cleanup | Owner EOF, helper exit zero, both owned process groups gone and private root removed |
| Screenshots | Actual English, Traditional Chinese and authenticated-download images visually reviewed and ignored |
| Unchanged Rust evidence | Accepted 7,314 passed, zero failed, 69 ignored across 475 workspace groups, with successful fmt/Clippy/all-targets build |

The Rust evidence is carried from `1833e98065b94e63a64263b235b7d7d0ca07e662`: every accepted source input
outside `frontend/macos/` is byte-identical. No new Rust workspace execution is
claimed. The native result bundle is `frontend/macos/.build/results-20261007-191948.xcresult`.
Actual runtime warnings (81), skip reasons, method
lists, frozen inputs and historical attempts are retained in the record. The
physical test skipped because Accessibility permission for physical key events
was unavailable; no Accessibility or other system setting was changed.

## Test development and ownership

An actual Xcode build caught the SFTP view missing from the unit target. The
runner then rejected writes into /private/tmp; UI-owned draft files now use its
own temporary directory. A parallel ranged HTTP fixture finished while the
human file panels were open, so the explicit Apply test now holds that fixture
unfinished with a per-test delay before checking paused state and exact resumed
bytes. The default delay used by existing tests remains unchanged.

Starting OpenSSH within the generated UI runner failed: xcrun cannot run in an
App Sandbox, and the actual daemon later reported its own pre-authentication
sandbox initialization was refused. The test harness now owns the ephemeral
loopback daemon outside the runner, supplies a read-only test-bundle lease,
closes its owner pipe and verifies reaping/root removal after xcodebuild exits.
Runner sandbox, production SSH configuration and host verification are unchanged.

The first successful download attempt failed at direct runner Keychain cleanup.
The final case uses the actual native credential Remove operation. Exactly
identified earlier fixture items and roots were cleaned without reading secrets;
those cleanup results and unsuccessful native attempts remain recorded.

## Remaining browser delivery

Owned loopback authentication does not establish remote SFTP/FTPS deployment
interoperability, other authentication modes or a Linux/Windows acceptance run.
Automatic response downloads, destination selection with preserved history,
vector PDF, physical printers/IME/VoiceOver, full storage partitioning, additional
accessibility and other [delivery milestones](MACOS_DELIVERY_PLAN.md) remain
open. The earlier family-emoji missing-glyph rendering remains unresolved.
Local ad hoc signing does not establish distribution signing or Intel execution.

Reviewed ignored screenshot `development/browser_core/phase-22-browser-shell-accessibility/artifacts/macos-native-sftp-files-final1.png`: SHA-256 `8ef55c073b4e84a8a5ab7918f74e460f00b650b2c261ec3844419f38862e011c`.

Reviewed ignored screenshot `development/browser_core/phase-22-browser-shell-accessibility/artifacts/macos-native-sftp-files-zh-final1.png`: SHA-256 `22d195073ea3f5c83626edbda67feea6cda3cdb75c8cad83e2e20f46ac0f9ba9`.

Reviewed ignored screenshot `development/browser_core/phase-22-browser-shell-accessibility/artifacts/macos-native-sftp-authenticated-key-final1.png`: SHA-256 `53071f2bcae59b3a1bbc42d1299182010f4dbf5987c11d1e17609a5eac0e9f57`.
