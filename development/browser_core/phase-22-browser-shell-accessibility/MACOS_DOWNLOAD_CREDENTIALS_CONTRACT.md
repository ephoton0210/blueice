# macOS native download credentials

The Downloads panel opens a native SwiftUI account-credential surface. A human
enters the same SFTP or explicit-FTPS URL used for a transfer, reviews the returned
account, and explicitly saves or removes a credential. Server-root URLs are
accepted for account management; actual downloads still require a file path.
Resolution performs no network request, starts no transfer, and reads no secret.

## Account and storage boundary

The owned downloads socket adds `ResolveCredentialTarget` and its metadata-only
`CredentialTarget` reply. The shared transfer parsers own scheme, host, default
or explicit port and percent-decoded username. Native code does not independently
normalize the hostname, IPv6 brackets, encoded username or Unicode. Account
identity is limited to SFTP/FTPS, positive ports, bounded host/username strings
and no control characters. Embedded passwords, queries, fragments, unsupported
schemes and over-limit input are refused with a fixed diagnostic. Debug output
redacts the entire resolver input, including malformed URLs.

Save/Remove use the existing SFTP-password, SFTP-private-key-passphrase and
FTPS-password requests on the user-owned socket. The existing platform store
uses separate service namespaces, endpoint host/port and account username.
No credential is stored in the browser's preferences, downloads catalog,
transfer records, page tree, screenshots, history or AI/MCP results. Secret
entry uses a native SecureField. The draft clears before sending a mutation,
on credential-kind changes, account edits and leaving the surface; reopening
never reads a saved password into the field. Swift String storage is not claimed
to provide guaranteed memory zeroization.

A reviewed identity retains the current downloads-session epoch. Save/Remove
reject stale, foreign-session or scheme-incompatible reviews. Actions are
serialized. An uncertain or interrupted mutation is not automatically retried;
a request already sent may have completed in Keychain after the view closed.
A fresh review is required after reconnecting. Success requires the matching
owned `Ok` response. Saved entries survive application quit until removed;
removing a missing entry is idempotent. Ordinary downloads and their confirmed
browser page remain unchanged while accounts are managed.

Server host-key and TLS certificate/hostname verification still precede secret
lookup in actual transfer code. Saving a password does not trust a server,
start a transfer, grant a permission or bypass Gatekeeper. Plain FTP remains
anonymous-only. The SFTP private-key file is separate process configuration;
this panel manages its passphrase and does not select or transmit that file.
Native private-key selection/configuration and authenticated live-server
acceptance remain delivery work.

## Validation requirements

Public Rust tests cover shared-parser identity, root/file URLs, IPv6, encoded
and Unicode accounts, defaults, refusal/redaction and the actual owned socket.
Native XCTest validates bounded replies and exact private command encoding, then
uses the actual bundled service and macOS Keychain metadata to test namespaces,
persistence, removal and stopped-session refusal. Metadata queries forbid
interactive authentication and do not read a saved secret. Fixtures use unique
accounts and clean only their own entries.

XCUITest drives native account review, SecureField masking, keyboard Save,
SFTP/FTPS kinds, invalid URLs, dismissal, normal relaunch, localized labels and
explicit removal. Screenshot attachments show the actual native surface after
clearing the secret. English and Traditional Chinese resources share format
arguments. Acceptance requires terminal native and Rust gates, frozen source
inputs, reviewed ignored screenshots, signatures and owned-process cleanup.
The dated [results](MACOS_DOWNLOAD_CREDENTIALS_RESULTS.md) retain unsuccessful
attempts and distinguish store/UI acceptance from live-server authentication.


## Acceptance

Complete native acceptance passed 235 methods, zero failures
and one existing physical Zhuyin skip. The final workspace passed
7,314 cases, zero failures and 69 existing ignored cases.
Exact scope, source differences, actual screenshots and historical unsuccessful
attempts are retained in [the dated results](MACOS_DOWNLOAD_CREDENTIALS_RESULTS.md).
These gates accept credential-store/UI behavior; they do not establish
private-key configuration, live-server authentication or the complete browser.
