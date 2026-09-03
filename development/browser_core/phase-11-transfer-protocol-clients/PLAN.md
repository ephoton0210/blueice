# Phase 11 — FTP/SFTP and Other Transfer Protocol Clients

[← Back to plan](../BROWSER_CORE_PLAN.md)

**Status**: Not started

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

HTTP (Phase 10) implements this via `reqwest`; FTP via `suppaftp` or `async-ftp`; SFTP via an `ssh2`- or `russh`-based crate — Phase 10's chunking/resume logic calls through this trait rather than knowing which protocol it's talking to.

**Credentials**: FTP/SFTP need stored auth (username/password, or an SSH key for SFTP) that HTTP downloads mostly don't — this needs a secure local credential store (ideally OS-keychain integration per platform eventually; an encrypted local vault as a nearer-term fallback). Not yet in the checklist below; flagging it here since it's easy to overlook until someone actually tries to save an SFTP password.

## Open questions

- **Protocol scope**: FTP and SFTP are named explicitly; whether this also covers FTPS, WebDAV, or others is undecided.
- **Relationship to Phase 10**: same transfer-manager subsystem with a pluggable protocol-backend interface (this phase supplies FTP/SFTP backends, Phase 10 supplies the HTTP backend and the shared chunking/resume/UI machinery), or a fully separate component — needs to be decided together with Phase 10, not independently.
- **Crate evaluation**: candidates exist (`suppaftp`/`async-ftp` for FTP, `russh`- or `ssh2`-based crates for SFTP) but haven't been evaluated against this project's actual needs (async support, maintenance status, licensing compatibility with MPL-2.0).

## Checklist

- [ ] Decide relationship to Phase 10 (shared subsystem vs. separate) — do this together with Phase 10, not in isolation
- [ ] Confirm protocol scope (FTP, SFTP, and what else if anything)
- [ ] Evaluate candidate Rust crates for each protocol
- [ ] Define the common transfer-backend interface these protocols implement, shared with Phase 10's HTTP backend (see the `TransferBackend` sketch above)
- [ ] Design the credential storage mechanism for FTP/SFTP auth
