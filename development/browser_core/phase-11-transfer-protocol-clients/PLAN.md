# Phase 11 — FTP/SFTP and Other Transfer Protocol Clients

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: In progress

## Objective

Support file-transfer protocols beyond HTTP(S) — FTP, SFTP explicitly named, comparable to FileZilla as a client.

## Design sketch

**A common backend trait, shared with Phase 10**, so its chunking/resume/UI machinery stays protocol-agnostic rather than being re-implemented per protocol:

```rust
trait TransferBackend {
    fn list_dir(&self, path: &str) -> Result<Vec<Entry>>;
    fn get(&self, path: &str, range: Option<ByteRange>) -> Result<ByteStream>;
    fn put(&self, path: &str, data: ByteStream) -> Result<()>;
}
```

HTTP (Phase 10) implements this via `ureq`; FTP via `suppaftp` or `async-ftp`; SFTP via an `ssh2`- or `russh`-based crate — Phase 10's chunking/resume logic calls through this trait rather than knowing which protocol it's talking to.

**Credentials**: FTP/SFTP need stored auth (username/password, or an SSH key for SFTP) that HTTP downloads mostly don't — this needs a secure local credential store (ideally OS-keychain integration per platform eventually; an encrypted local vault as a nearer-term fallback). Not yet in the checklist below; flagging it here since it's easy to overlook until someone actually tries to save an SFTP password.

## Decisions

- **Shared download-only interface**: `TransferBackend` has `probe` and
  `get(range)`. It returns an owned byte stream and proves that any requested
  range is exactly the range of the resource that was probed. The common
  scheduler owns splitting, retries, writes, checkpointing, and resume. This
  is intentionally not a premature remote-files API: list/upload require a
  separate user-facing capability and authorization model.
- **Protocol scope**: HTTP(S) remains the existing backend; SFTP is the first
  additional backend. FTP is a legacy compatibility protocol, while explicit
  FTPS is the only password-bearing FTP mode that BlueIce will offer. Plain
  FTP is restricted to anonymous downloads; WebDAV is deferred.
- **SFTP candidate**: use the synchronous `ssh2` binding (MIT licensed,
  binding to libssh2). Its blocking `Read + Seek` SFTP files fit the transfer
  workers directly; a Tokio-only client would add a second scheduler just to
  bridge to this engine.
- **FTP candidate**: do **not** use `suppaftp` at this time. Its currently
  unpatched [RustSec advisory RUSTSEC-2026-0271](https://rustsec.org/advisories/RUSTSEC-2026-0271.html)
  permits CRLF command injection when a caller supplies credentials or paths.
  `ftp` is unmaintained and `async-ftp` has no advantage for this synchronous
  engine. This is a safety gate, not a waiver: the FTP/FTPS backend stays
  unavailable until a patched dependency is available or a narrowly audited
  implementation exists.
- **Credential boundary**: URLs may select an endpoint and optional username,
  but never carry a password or private-key material. A normalized
  `(scheme, host, port, username)` credential reference is persisted with the
  transfer; the secret itself lives only in the OS credential store. The IPC
  uses opaque references after a local credential has been saved, so transfer
  records, logs, sidecars, and MCP output never contain a secret. SSH host
  keys must be checked against a known-hosts store before SFTP authentication.

## Checklist

- [x] Decide relationship to Phase 10 (shared subsystem vs. separate) — decided with Phase 10: one shared subsystem, with this phase supplying FTP/SFTP backends. The `TransferBackend` trait itself is deliberately not defined yet (with one implementation its shape would be a guess); Phase 10's engine keeps its ranged fetch behind one function boundary (`blueice_net::download::http`) so extracting it is mechanical once a second backend exists
- [x] Confirm protocol scope (FTP, SFTP, and what else if anything)
- [x] Evaluate candidate Rust crates for each protocol
- [x] Define the common transfer-backend interface these protocols implement, shared with Phase 10's HTTP backend (see the `TransferBackend` sketch above)
- [x] Design the credential storage mechanism for FTP/SFTP auth
- [x] Implement the SFTP backend, known-host verification, and SSH-agent authentication (no password is accepted in a URL or recorded in transfer state)
- [ ] Add OS-keychain credential references for SFTP passwords and encrypted private-key passphrases
- [ ] Add a tested, audited explicit-FTPS backend once its dependency risk is resolved
