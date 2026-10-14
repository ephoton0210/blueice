# Native SFTP file configuration contract

Status: implemented and accepted. Complete native acceptance passed 245 methods
with zero failures and one existing physical Zhuyin skip. See
[the dated results](MACOS_SFTP_FILES_RESULTS.md) for exact scope and evidence.
This increment continues the macOS browser delivery plan and does not complete
the remaining browser requirements.

## Human file selection and persistence

Downloads offers SFTP files in the existing SwiftUI sheet. AppKit NSOpenPanel
selects one readable regular local file for SSH known hosts or the SFTP private
key. Directories, missing files, aliases, endpoint symlinks, remote URLs and
control-character paths are refused. Parent paths are canonicalized; macOS may
represent `/private/tmp` as `/tmp`. The selected file remains a draft until the
human explicitly applies it. Cancelling the chooser, returning to Downloads or
closing the sheet discards unsaved choices.

Version 1 of `browser.downloads.sftp.configuration` stores only absolute file
paths, with a 4096-byte limit per path. It contains no file contents or
passphrases. Unknown versions and malformed saved values remain visible errors;
they do not silently become defaults. The human can choose replacement files
or explicitly restore the defaults. A configured file that disappears or
becomes unreadable prevents download-service startup until repaired.

The default known-hosts file remains the existing backend default,
`~/.ssh/known_hosts`. No private-key file means no configured key file; it still
permits the backend's existing SSH-agent and stored-password authentication.
An explicitly chosen private key is passed through `--sftp-private-key`, and
known hosts through `--sftp-known-hosts`, to the owned download process. These
paths are not page, assistant or download protocol fields. Private-key bytes are
read by the existing SFTP transport only when authentication requires them.
Saved key passphrases remain in the existing macOS Keychain namespace managed
by Downloads > Credentials.

## Apply, ownership and interrupted downloads

The UI explains that Apply restarts the download service and pauses unfinished
downloads. Applying or restoring settings serializes against connection startup,
download mutations and credential mutations. The owned download manager
receives Shutdown and is reaped before a fresh manager opens the same catalog.
The browser core, windows and page documents remain owned by the existing
workspace. Interrupted transfers remain paused, including after normal browser
quit/relaunch, until the human explicitly chooses Resume.

A fresh download-session epoch invalidates previously reviewed credential
identities. Stale replies and old credential mutations cannot act on the new
service. Closing the configuration view cancels its captured operation and file
panel; an operation that already sent Shutdown may have paused downloads, so the
UI makes no rollback promise after submission. Stopping the workspace prevents
configuration from starting another manager.

Host-key verification remains required and uses the existing backend policy.
Choosing or applying files grants no server trust and performs no automatic
credential retry. File metadata is validated before persistence and again
before spawning the service; the existing transport handles changes or errors
when subsequently opening a configured file. No stable inode lock or guaranteed
Swift-memory zeroization is claimed. Owned loopback authentication is exercised
separately and does not establish remote-server interoperability.

## Required acceptance

Foundation tests cover the versioned preference codec, selected-file identity,
invalid files, explicit persistence, reopening and restoration. Actual-service
XCTest must verify graceful checkpointing, a new download process, the unchanged
core process, paused byte counts, rejected drafts, stale reviewed accounts,
explicit resume, stopped-workspace refusal and repair of invalid saved settings.

XCUITest must exercise the actual NSOpenPanel, explicit Apply, a transfer active
at application time, normal quit/relaunch, restored defaults, explicit Resume,
cancelled chooser and dismissed draft, and English/Traditional Chinese labels.
Screenshots must be visually inspected and remain ignored verification files.
The complete native regression suite, unchanged source/product audit, signatures,
universal application architectures and owned-process cleanup are required
before this increment is accepted, committed and pushed. Type checking does not
establish UI or live-server acceptance.

Authenticated live-server interoperability, automatic response downloads,
destination selection, vector PDF, physical printer/IME/VoiceOver and the
remaining macOS delivery milestones continue separately.


## Actual encrypted-key authentication acceptance

The additional XCUITest must choose an actual generated encrypted RSA private
key and the generated known-hosts file through NSOpenPanel, explicitly Apply,
review the account again in Credentials, save its passphrase through the native
SecureField/Keychain flow, and complete an SFTP transfer with exact byte equality.
The owned app has no production SSH agent socket for this test. Metadata-only
noninteractive queries must refuse to overwrite a pre-existing Keychain tuple;
normal cleanup removes only the item the fixture created through the native
credential surface and verifies metadata-only absence. The test harness stops
and reaps its owned server and removes its private root after xcodebuild exits.

`frontend/macos/test.sh` starts the daemon outside the sandboxed XCUITest runner
through `TestSupport/run-sftp-ui-tests.py`. The runner reads an immutable lease
from its test-bundle Info.plist. OpenSSH must initialize its own pre-authentication
sandbox; starting it inside the runner sandbox is refused by macOS. No runner or
system sandbox policy is disabled. The harness retains the daemon owner pipe,
closes it on completion or interruption, verifies process-group exit and removes
the temporary root. Its actual cleanup record is part of the native test log.

The fixture is an ephemeral unprivileged OpenSSH daemon bound only to
127.0.0.1, with generated keys, public-key-only authentication and forced
internal-sftp. Its root is 0700; generated private keys and readiness metadata
are 0600. Other fixture files remain confined by the private root. The daemon disables
StrictModes solely because /private/tmp has a shared writable parent; no system
or user SSH configuration changes. Client host-key verification remains strict
and matches the generated key. Earlier independent CLI fixture prerequisites validated real
file transfer, the original key's encryption and byte preservation, owner EOF,
process cleanup and removal of the temporary root. This prerequisite does not
establish BlueIce or Keychain acceptance; the native end-to-end case and complete regression suite must pass.
Successful owned loopback authentication does not establish interoperability
with remote deployments, FTPS servers or other authentication arrangements.
