// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The wire protocol between `backend/downloads` and its clients -- the
//! MCP adapter, `core` (for `about:downloads`), and `frontend`
//! (`phase-10-download-manager/PLAN.md`'s "Wire protocol"). Lives inside
//! `blueice-ipc` for the same reason [`crate::gatekeeper`] and
//! [`crate::extension`] do: to reuse this crate's private length-
//! prefixed-JSON framing primitives ([`crate::write_framed`]/
//! [`crate::read_frame_bytes`]) -- a descendant module of the crate
//! root can already reach them.
//!
//! A separate protocol rather than new [`crate::ClientMessage`]
//! variants: `core` would otherwise have to learn, and relay, a
//! vocabulary only the downloads process serves, and downloads would be
//! coupled to [`crate::PROTOCOL_VERSION`]'s all-or-nothing bumps.
//!
//! Shape: a **long-lived connection** with a `Hello`/`protocol_version`
//! handshake (unlike [`crate::gatekeeper`]'s one-shot checks), and an
//! envelope carrying an optional `request_id` -- like the client-facing
//! protocol -- because a subscribed connection receives *pushed*
//! [`DownloadsReply::Updated`] messages interleaved with replies, and a
//! caller has to tell the two apart. Pushes carry no `request_id`.
//!
//! Forward compatibility follows [`crate::ClientMessage`]'s rules: an
//! unrecognized request/reply variant fails soft to `Unknown` (keeping
//! its `request_id`), every field added to [`TransferInfo`] later is
//! `#[serde(default)]`, and an unrecognized *value* of any enum inside a
//! record degrades that one field to `Unknown` instead of losing the
//! whole record. Only syntactically malformed JSON is an `io::Error`.
//!
//! [`TransferInfo`] is also what an AI agent reads verbatim in MCP tool
//! results, so its enum values are snake_case strings
//! (`"awaiting_clearance"`), not Rust-shaped ones.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// The whole-protocol version of this module -- separate from
/// [`crate::PROTOCOL_VERSION`] (a different protocol with a different
/// set of peers). Bumped only on a breaking change; adding a request or
/// reply variant, or a `#[serde(default)]` field, does not bump it.
pub const DOWNLOADS_PROTOCOL_VERSION: u32 = 1;

/// How many pushed [`DownloadsReply::Updated`] messages a
/// [`DownloadsClient`] keeps while it isn't being asked for them. A
/// client that subscribes but rarely drains must not grow without
/// bound; the oldest are dropped, since the newest state of a transfer
/// supersedes an older one.
pub const MAX_BUFFERED_UPDATES: usize = 1024;

/// Where a transfer is in its life. Snake_case on the wire and in
/// [`fmt::Display`]; `Unknown` is what a state added by a newer peer
/// reads as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    /// Accepted, waiting for a free slot (only so many transfers run at once).
    #[default]
    Queued,
    /// Waiting on, or in the middle of, the gatekeeper's review and the
    /// probe that feeds it -- indistinguishable from `Queued` or `Failed`
    /// without its own state.
    AwaitingClearance,
    Active,
    Paused,
    Completed,
    Failed,
    Cancelled,
    /// The gatekeeper refused (or, fail-closed, couldn't be reached);
    /// see [`TransferInfo::blocked`].
    Blocked,
    #[serde(other)]
    Unknown,
}

impl TransferState {
    /// Whether the transfer has stopped for good (no further progress
    /// without a new request). `Paused` is not terminal -- it resumes.
    pub fn is_terminal(self) -> bool {
        matches!(self, TransferState::Completed | TransferState::Failed | TransferState::Cancelled | TransferState::Blocked)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TransferState::Queued => "queued",
            TransferState::AwaitingClearance => "awaiting_clearance",
            TransferState::Active => "active",
            TransferState::Paused => "paused",
            TransferState::Completed => "completed",
            TransferState::Failed => "failed",
            TransferState::Cancelled => "cancelled",
            TransferState::Blocked => "blocked",
            TransferState::Unknown => "unknown",
        }
    }
}

impl fmt::Display for TransferState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a transfer isn't segmented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SingleStreamReason {
    /// The server answered the range probe with a plain `200`.
    ServerIgnoresRange,
    /// No `Content-Length`, so there is nothing to split.
    UnknownLength,
    #[default]
    #[serde(other)]
    Unknown,
}

/// How the bytes are being fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransferMode {
    /// Not probed yet.
    #[default]
    Undetermined,
    /// Several concurrent ranged requests into one pre-allocated file.
    Segmented,
    /// One sequential stream; resuming after a pause restarts from byte 0.
    SingleStream {
        #[serde(default)]
        reason: SingleStreamReason,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentState {
    #[default]
    Pending,
    Active,
    Done,
    /// Failed and waiting out its backoff before another attempt.
    Retrying,
    #[serde(other)]
    Unknown,
}

/// One byte range of a segmented transfer: `[start, end)`, of which the
/// first `completed` bytes are on disk.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SegmentInfo {
    pub start: u64,
    pub end: u64,
    pub completed: u64,
    pub state: SegmentState,
}

/// Why the gatekeeper stopped a transfer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BlockedInfo {
    pub reason: String,
    pub category: String,
}

/// One entry of a transfer's bounded event log -- what an AI agent reads
/// to learn *why* a transfer is slow or failed, not just that it is.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TransferEvent {
    /// Unix time, milliseconds.
    pub at_ms: u64,
    pub message: String,
}

/// The one record every consumer of a transfer reads -- the AI-facing MCP
/// tools and the human-facing `about:downloads` page alike.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TransferInfo {
    pub id: u64,
    /// As requested.
    pub url: String,
    /// After redirects, once probed.
    pub final_url: Option<String>,
    /// Absolute path of the finished file.
    pub dest_path: String,
    pub state: TransferState,
    /// `None` until known, or when the server doesn't say.
    pub total_bytes: Option<u64>,
    pub completed_bytes: u64,
    /// Smoothed throughput.
    pub speed_bps: u64,
    /// `None` when the speed or the total is unknown.
    pub eta_secs: Option<u64>,
    /// Connections currently downloading.
    pub connections: u32,
    pub mode: TransferMode,
    /// Whether pausing and resuming keeps the bytes already fetched: the
    /// server gave a range-capable, validator-bearing answer. `false`
    /// means a pause restarts from byte 0 -- reported up front, not
    /// discovered afterward.
    pub resume_safe: bool,
    pub segments: Vec<SegmentInfo>,
    /// Failed attempts so far, across all segments.
    pub retries: u32,
    pub last_error: Option<String>,
    /// Set for [`TransferState::Blocked`].
    pub blocked: Option<BlockedInfo>,
    pub content_type: Option<String>,
    /// Unix time, milliseconds.
    pub created_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    /// Increases with every change, so a client can drop a stale update.
    pub generation: u64,
    /// The most recent notable events, oldest first (bounded).
    pub events: Vec<TransferEvent>,
}

impl TransferInfo {
    /// `completed / total` in `[0, 1]`, or `None` when there is no usable
    /// total to divide by.
    pub fn fraction_complete(&self) -> Option<f64> {
        match self.total_bytes {
            Some(total) if total > 0 => Some((self.completed_bytes as f64 / total as f64).clamp(0.0, 1.0)),
            _ => None,
        }
    }
}

const BYTE_UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

/// `1234567` -> `1.2 MiB`: binary units, one decimal (rounded half up),
/// moving up a unit rather than printing `1024.0 KiB`. Shared by every
/// consumer that shows a transfer to a person or an agent -- the MCP
/// tools' summaries and the `about:downloads` page -- so they never
/// disagree about how big a file is.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut unit = 1usize;
    loop {
        let divisor = 1u128 << (10 * unit);
        let tenths = (u128::from(bytes) * 10 + divisor / 2) / divisor;
        if tenths < 10_240 || unit == BYTE_UNITS.len() - 1 {
            return format!("{}.{} {}", tenths / 10, tenths % 10, BYTE_UNITS[unit]);
        }
        unit += 1;
    }
}

pub fn format_speed(bytes_per_second: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_second))
}

/// `80` -> `1 min 20 s`; a unit that no longer matters at that scale is dropped.
pub fn format_duration(seconds: u64) -> String {
    let part = |n: u64, unit: &str| format!("{n} {unit}");
    let join = |major: String, minor: u64, unit: &str| if minor == 0 { major } else { format!("{major} {}", part(minor, unit)) };
    match seconds {
        0..=59 => part(seconds, "s"),
        60..=3599 => join(part(seconds / 60, "min"), seconds % 60, "s"),
        3600..=86_399 => join(part(seconds / 3600, "h"), (seconds % 3600) / 60, "min"),
        _ => join(part(seconds / 86_400, "d"), (seconds % 86_400) / 3600, "h"),
    }
}

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    /// The request itself was unacceptable (a bad URL, a destination
    /// outside the downloads directory, ...).
    InvalidRequest,
    /// Valid, but not in the transfer's current state (pausing a
    /// completed transfer, ...).
    InvalidState,
    UnsupportedVersion,
    Internal,
    #[serde(other)]
    Unknown,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::NotFound => "not_found",
            ErrorCode::InvalidRequest => "invalid_request",
            ErrorCode::InvalidState => "invalid_state",
            ErrorCode::UnsupportedVersion => "unsupported_version",
            ErrorCode::Internal => "internal",
            ErrorCode::Unknown => "unknown",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a client asks the downloads process to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DownloadsRequest {
    /// Sent once, first, on a fresh connection.
    Hello { protocol_version: u32 },
    /// Queue a download. `dest` is a path *relative to the downloads
    /// directory* (absolute paths and `..` are refused); `None` derives a
    /// name from the response or the URL. Replied to with
    /// [`DownloadsReply::Started`] as soon as the transfer is queued --
    /// progress is read afterward, not waited for.
    Start {
        url: String,
        #[serde(default)]
        dest: Option<String>,
        #[serde(default)]
        overwrite: bool,
    },
    /// Every known transfer, optionally only those in `state`.
    List {
        #[serde(default)]
        state: Option<TransferState>,
    },
    Get { id: u64 },
    Pause { id: u64 },
    /// Re-enters review: a verdict can change between a pause and a resume.
    Resume { id: u64 },
    /// Stops the transfer and discards its partial files.
    Cancel { id: u64 },
    /// Drops a finished transfer from history (never a running one).
    Remove { id: u64 },
    /// Store an SFTP password in the platform credential store. The password
    /// is sent only over this user-owned Unix socket, is never persisted in
    /// the downloads database, and is never included in a reply.
    SetSftpPassword {
        host: String,
        #[serde(default = "default_sftp_port")]
        port: u16,
        username: String,
        password: String,
    },
    /// Remove the saved password for this SFTP endpoint and user.
    RemoveSftpPassword {
        host: String,
        #[serde(default = "default_sftp_port")]
        port: u16,
        username: String,
    },
    /// Store an encrypted SFTP private key's passphrase in the platform
    /// credential store. The key remains a local process configuration and
    /// is never sent on this socket.
    SetSftpPrivateKeyPassphrase {
        host: String,
        #[serde(default = "default_sftp_port")]
        port: u16,
        username: String,
        passphrase: String,
    },
    /// Remove the saved passphrase for an SFTP private key at this endpoint.
    RemoveSftpPrivateKeyPassphrase {
        host: String,
        #[serde(default = "default_sftp_port")]
        port: u16,
        username: String,
    },
    /// Store an explicit-FTPS password in the platform credential store.
    /// Plain FTP is anonymous-only, so it has no password request.
    SetFtpsPassword {
        host: String,
        #[serde(default = "default_ftps_port")]
        port: u16,
        username: String,
        password: String,
    },
    /// Remove the saved password for this explicit-FTPS endpoint and user.
    RemoveFtpsPassword {
        host: String,
        #[serde(default = "default_ftps_port")]
        port: u16,
        username: String,
    },
    /// After this, every change to any transfer is pushed as
    /// [`DownloadsReply::Updated`] on this connection.
    Subscribe,
    Shutdown,
    /// See [`crate::ClientMessage::Unknown`] -- the same fail-soft fallback.
    #[serde(other)]
    Unknown,
}

fn default_sftp_port() -> u16 {
    22
}

fn default_ftps_port() -> u16 {
    21
}

/// The downloads process's message to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DownloadsReply {
    Hello { protocol_version: u32 },
    /// Reply to [`DownloadsRequest::Start`]: the newly queued transfer.
    Started(TransferInfo),
    Transfers(Vec<TransferInfo>),
    /// Reply to `Get`, `Pause`, `Resume`, and `Cancel`: the transfer as
    /// it now stands.
    Transfer(TransferInfo),
    /// Reply to `Remove`, `Subscribe`, and `Shutdown`.
    Ok,
    Error { code: ErrorCode, message: String },
    /// Pushed to a subscriber (no `request_id`) when a transfer changes.
    Updated(TransferInfo),
    #[serde(other)]
    Unknown,
}

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_id: Option<u64>,
    message: T,
}

fn write_enveloped<W: Write, T: Serialize>(w: &mut W, request_id: Option<u64>, message: &T) -> io::Result<()> {
    crate::write_framed(w, &Envelope { request_id, message })
}

/// Reads one frame as an [`Envelope`], falling back to `unknown` --
/// preserving `request_id` if it can still be pulled out of the raw JSON
/// -- when the JSON is well-formed but isn't a message this build knows
/// (see [`crate::ClientMessage::Unknown`] for why `#[serde(other)]`
/// alone can't cover a data-carrying unrecognized variant).
fn read_enveloped<R: Read, T: DeserializeOwned>(r: &mut R, unknown: T) -> io::Result<(Option<u64>, T)> {
    let buf = crate::read_frame_bytes(r)?;
    let value: serde_json::Value = serde_json::from_slice(&buf).map_err(io::Error::other)?;
    let request_id = value.get("request_id").and_then(serde_json::Value::as_u64);
    match serde_json::from_value::<Envelope<T>>(value) {
        Ok(envelope) => Ok((envelope.request_id, envelope.message)),
        Err(_) => Ok((request_id, unknown)),
    }
}

pub fn write_downloads_request<W: Write>(w: &mut W, request_id: Option<u64>, msg: &DownloadsRequest) -> io::Result<()> {
    write_enveloped(w, request_id, msg)
}

pub fn read_downloads_request<R: Read>(r: &mut R) -> io::Result<(Option<u64>, DownloadsRequest)> {
    read_enveloped(r, DownloadsRequest::Unknown)
}

pub fn write_downloads_reply<W: Write>(w: &mut W, request_id: Option<u64>, msg: &DownloadsReply) -> io::Result<()> {
    write_enveloped(w, request_id, msg)
}

pub fn read_downloads_reply<R: Read>(r: &mut R) -> io::Result<(Option<u64>, DownloadsReply)> {
    read_enveloped(r, DownloadsReply::Unknown)
}

/// Where the downloads process listens, and where its clients connect by
/// default in production. Same per-user convention as
/// [`crate::gatekeeper::default_gatekeeper_socket_path`] (two users, or
/// two independent BlueIce sessions, never collide), a distinct filename.
/// Tests thread an explicit path through instead.
pub fn default_downloads_socket_path() -> PathBuf {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("blueice"),
        None => std::env::temp_dir().join(format!("blueice-{}", unsafe { libc_getuid() })),
    };
    dir.join("downloads.sock")
}

// Duplicated from the sibling protocol modules' identical helper rather
// than shared -- see `gatekeeper.rs` for why.
unsafe fn libc_getuid() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| status.lines().find_map(|line| line.strip_prefix("Uid:")).and_then(|rest| rest.split_whitespace().next()).and_then(|s| s.parse().ok()))
        .unwrap_or_else(std::process::id)
}

/// Why a [`DownloadsClient`] call failed.
#[derive(Debug)]
pub enum ClientError {
    /// The connection failed, or the peer hung up.
    Io(io::Error),
    /// The downloads process understood the request and refused it.
    Remote { code: ErrorCode, message: String },
    /// A reply of a shape this call can't make sense of.
    Unexpected(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "talking to the downloads process failed: {e}"),
            ClientError::Remote { code, message } => write!(f, "downloads process refused the request ({code}): {message}"),
            ClientError::Unexpected(what) => write!(f, "unexpected reply from the downloads process: {what}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(e: io::Error) -> Self {
        ClientError::Io(e)
    }
}

/// A blocking client for the downloads protocol, generic over the stream
/// (`UnixStream` in production, a socket pair in tests) so the MCP
/// adapter, `core`, and `frontend` share one implementation instead of
/// each re-deriving request-id matching and push handling.
///
/// Every call is tagged with a fresh `request_id` and waits for the reply
/// carrying it. Anything else that arrives meanwhile -- pushed
/// [`DownloadsReply::Updated`] messages, or a reply meant for another
/// client sharing the connection -- is set aside, never mistaken for the
/// answer; pushes are kept (bounded, see [`MAX_BUFFERED_UPDATES`]) for
/// [`Self::take_buffered_updates`]/[`Self::next_update`].
#[derive(Debug)]
pub struct DownloadsClient<S> {
    stream: S,
    next_request_id: u64,
    updates: VecDeque<TransferInfo>,
}

impl<S: Read + Write> DownloadsClient<S> {
    /// Performs the `Hello` handshake on a fresh connection.
    pub fn connect(stream: S) -> Result<Self, ClientError> {
        let mut client = DownloadsClient { stream, next_request_id: 1, updates: VecDeque::new() };
        match client.call(DownloadsRequest::Hello { protocol_version: DOWNLOADS_PROTOCOL_VERSION })? {
            DownloadsReply::Hello { protocol_version } if protocol_version == DOWNLOADS_PROTOCOL_VERSION => Ok(client),
            other => Err(ClientError::Unexpected(format!("expected a Hello for protocol version {DOWNLOADS_PROTOCOL_VERSION}, got {other:?}"))),
        }
    }

    fn call(&mut self, request: DownloadsRequest) -> Result<DownloadsReply, ClientError> {
        let id = self.next_request_id;
        self.next_request_id += 1;
        write_downloads_request(&mut self.stream, Some(id), &request)?;
        loop {
            match read_downloads_reply(&mut self.stream)? {
                (Some(reply_id), reply) if reply_id == id => {
                    return match reply {
                        DownloadsReply::Error { code, message } => Err(ClientError::Remote { code, message }),
                        other => Ok(other),
                    };
                }
                (None, DownloadsReply::Updated(info)) => self.buffer_update(info),
                // Another client's reply, or something this build can't read.
                _ => {}
            }
        }
    }

    fn buffer_update(&mut self, info: TransferInfo) {
        if self.updates.len() >= MAX_BUFFERED_UPDATES {
            self.updates.pop_front();
        }
        self.updates.push_back(info);
    }

    fn transfer_reply(reply: DownloadsReply) -> Result<TransferInfo, ClientError> {
        match reply {
            DownloadsReply::Transfer(info) | DownloadsReply::Started(info) => Ok(info),
            other => Err(ClientError::Unexpected(format!("expected a transfer, got {other:?}"))),
        }
    }

    fn ok_reply(reply: DownloadsReply) -> Result<(), ClientError> {
        match reply {
            DownloadsReply::Ok => Ok(()),
            other => Err(ClientError::Unexpected(format!("expected Ok, got {other:?}"))),
        }
    }

    pub fn start(&mut self, url: &str, dest: Option<&str>, overwrite: bool) -> Result<TransferInfo, ClientError> {
        let reply = self.call(DownloadsRequest::Start { url: url.to_string(), dest: dest.map(str::to_string), overwrite })?;
        Self::transfer_reply(reply)
    }

    pub fn list(&mut self, state: Option<TransferState>) -> Result<Vec<TransferInfo>, ClientError> {
        match self.call(DownloadsRequest::List { state })? {
            DownloadsReply::Transfers(transfers) => Ok(transfers),
            other => Err(ClientError::Unexpected(format!("expected a transfer list, got {other:?}"))),
        }
    }

    pub fn get(&mut self, id: u64) -> Result<TransferInfo, ClientError> {
        Self::transfer_reply(self.call(DownloadsRequest::Get { id })?)
    }

    pub fn pause(&mut self, id: u64) -> Result<TransferInfo, ClientError> {
        Self::transfer_reply(self.call(DownloadsRequest::Pause { id })?)
    }

    pub fn resume(&mut self, id: u64) -> Result<TransferInfo, ClientError> {
        Self::transfer_reply(self.call(DownloadsRequest::Resume { id })?)
    }

    pub fn cancel(&mut self, id: u64) -> Result<TransferInfo, ClientError> {
        Self::transfer_reply(self.call(DownloadsRequest::Cancel { id })?)
    }

    pub fn remove(&mut self, id: u64) -> Result<(), ClientError> {
        Self::ok_reply(self.call(DownloadsRequest::Remove { id })?)
    }

    /// Stores `password` in the local operating system credential store. It
    /// is deliberately a non-idempotent call: if its reply is lost, a caller
    /// must decide whether to retry rather than silently repeating a secret
    /// write.
    pub fn set_sftp_password(&mut self, host: &str, port: u16, username: &str, password: &str) -> Result<(), ClientError> {
        Self::ok_reply(self.call(DownloadsRequest::SetSftpPassword { host: host.to_string(), port, username: username.to_string(), password: password.to_string() })?)
    }

    pub fn remove_sftp_password(&mut self, host: &str, port: u16, username: &str) -> Result<(), ClientError> {
        Self::ok_reply(self.call(DownloadsRequest::RemoveSftpPassword { host: host.to_string(), port, username: username.to_string() })?)
    }

    /// Stores the passphrase for the private key configured with the local
    /// downloads process. The passphrase is never returned.
    pub fn set_sftp_private_key_passphrase(&mut self, host: &str, port: u16, username: &str, passphrase: &str) -> Result<(), ClientError> {
        Self::ok_reply(self.call(DownloadsRequest::SetSftpPrivateKeyPassphrase { host: host.to_string(), port, username: username.to_string(), passphrase: passphrase.to_string() })?)
    }

    pub fn remove_sftp_private_key_passphrase(&mut self, host: &str, port: u16, username: &str) -> Result<(), ClientError> {
        Self::ok_reply(self.call(DownloadsRequest::RemoveSftpPrivateKeyPassphrase { host: host.to_string(), port, username: username.to_string() })?)
    }

    /// Stores an explicit-FTPS password in the local operating system
    /// credential store. As with SFTP, the password is never returned.
    pub fn set_ftps_password(&mut self, host: &str, port: u16, username: &str, password: &str) -> Result<(), ClientError> {
        Self::ok_reply(self.call(DownloadsRequest::SetFtpsPassword { host: host.to_string(), port, username: username.to_string(), password: password.to_string() })?)
    }

    pub fn remove_ftps_password(&mut self, host: &str, port: u16, username: &str) -> Result<(), ClientError> {
        Self::ok_reply(self.call(DownloadsRequest::RemoveFtpsPassword { host: host.to_string(), port, username: username.to_string() })?)
    }

    pub fn subscribe(&mut self) -> Result<(), ClientError> {
        Self::ok_reply(self.call(DownloadsRequest::Subscribe)?)
    }

    /// Drains the pushes set aside during earlier calls, oldest first.
    pub fn take_buffered_updates(&mut self) -> Vec<TransferInfo> {
        self.updates.drain(..).collect()
    }

    /// The next pushed update -- a buffered one first, otherwise blocking
    /// on the stream until one arrives. Meant for a subscribed connection.
    pub fn next_update(&mut self) -> Result<TransferInfo, ClientError> {
        if let Some(info) = self.updates.pop_front() {
            return Ok(info);
        }
        loop {
            if let (_, DownloadsReply::Updated(info)) = read_downloads_reply(&mut self.stream)? {
                return Ok(info);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::thread;

    fn sample_info(id: u64) -> TransferInfo {
        TransferInfo {
            id,
            url: "https://example.com/big.iso".to_string(),
            final_url: Some("https://cdn.example.com/big.iso".to_string()),
            dest_path: "/home/u/Downloads/BlueIce/big.iso".to_string(),
            state: TransferState::Active,
            total_bytes: Some(1_000),
            completed_bytes: 400,
            speed_bps: 2_048,
            eta_secs: Some(3),
            connections: 2,
            mode: TransferMode::Segmented,
            resume_safe: true,
            segments: vec![
                SegmentInfo { start: 0, end: 500, completed: 400, state: SegmentState::Active },
                SegmentInfo { start: 500, end: 1_000, completed: 0, state: SegmentState::Retrying },
            ],
            retries: 1,
            last_error: Some("connection reset".to_string()),
            blocked: None,
            content_type: Some("application/octet-stream".to_string()),
            created_at_ms: 1_700_000_000_000,
            finished_at_ms: None,
            generation: 9,
            events: vec![TransferEvent { at_ms: 1_700_000_000_500, message: "probe: server supports ranges".to_string() }],
        }
    }

    fn every_request() -> Vec<DownloadsRequest> {
        vec![
            DownloadsRequest::Hello { protocol_version: DOWNLOADS_PROTOCOL_VERSION },
            DownloadsRequest::Start { url: "https://example.com/a.bin".to_string(), dest: Some("sub/a.bin".to_string()), overwrite: true },
            DownloadsRequest::Start { url: "https://example.com/a.bin".to_string(), dest: None, overwrite: false },
            DownloadsRequest::List { state: None },
            DownloadsRequest::List { state: Some(TransferState::Paused) },
            DownloadsRequest::Get { id: 3 },
            DownloadsRequest::Pause { id: 3 },
            DownloadsRequest::Resume { id: 3 },
            DownloadsRequest::Cancel { id: 3 },
            DownloadsRequest::Remove { id: 3 },
            DownloadsRequest::SetSftpPassword { host: "files.example.test".to_string(), port: 2222, username: "alice".to_string(), password: "not-a-real-secret".to_string() },
            DownloadsRequest::RemoveSftpPassword { host: "files.example.test".to_string(), port: 2222, username: "alice".to_string() },
            DownloadsRequest::SetSftpPrivateKeyPassphrase { host: "files.example.test".to_string(), port: 2222, username: "alice".to_string(), passphrase: "not-a-real-secret".to_string() },
            DownloadsRequest::RemoveSftpPrivateKeyPassphrase { host: "files.example.test".to_string(), port: 2222, username: "alice".to_string() },
            DownloadsRequest::SetFtpsPassword { host: "files.example.test".to_string(), port: 2121, username: "alice".to_string(), password: "not-a-real-secret".to_string() },
            DownloadsRequest::RemoveFtpsPassword { host: "files.example.test".to_string(), port: 2121, username: "alice".to_string() },
            DownloadsRequest::Subscribe,
            DownloadsRequest::Shutdown,
        ]
    }

    fn every_reply() -> Vec<DownloadsReply> {
        let mut blocked = sample_info(2);
        blocked.state = TransferState::Blocked;
        blocked.blocked = Some(BlockedInfo { reason: "executable from an untrusted origin".to_string(), category: "dangerous-file-type".to_string() });
        vec![
            DownloadsReply::Hello { protocol_version: DOWNLOADS_PROTOCOL_VERSION },
            DownloadsReply::Started(sample_info(1)),
            DownloadsReply::Transfers(vec![sample_info(1), blocked]),
            DownloadsReply::Transfers(Vec::new()),
            DownloadsReply::Transfer(sample_info(1)),
            DownloadsReply::Ok,
            DownloadsReply::Error { code: ErrorCode::NotFound, message: "no transfer 9".to_string() },
            DownloadsReply::Updated(sample_info(1)),
        ]
    }

    #[test]
    fn every_request_round_trips_and_keeps_its_request_id() {
        for (i, req) in every_request().into_iter().enumerate() {
            for request_id in [None, Some(i as u64 + 1)] {
                let (mut a, mut b) = UnixStream::pair().unwrap();
                write_downloads_request(&mut a, request_id, &req).unwrap();
                assert_eq!(read_downloads_request(&mut b).unwrap(), (request_id, req.clone()));
            }
        }
    }

    #[test]
    fn password_requests_without_ports_use_their_protocol_defaults() {
        for (message, expected) in [
            (
                serde_json::json!({"SetSftpPassword": {"host": "files.example.test", "username": "alice", "password": "not-a-real-secret"}}),
                DownloadsRequest::SetSftpPassword { host: "files.example.test".to_string(), port: 22, username: "alice".to_string(), password: "not-a-real-secret".to_string() },
            ),
            (
                serde_json::json!({"SetSftpPrivateKeyPassphrase": {"host": "files.example.test", "username": "alice", "passphrase": "not-a-real-secret"}}),
                DownloadsRequest::SetSftpPrivateKeyPassphrase { host: "files.example.test".to_string(), port: 22, username: "alice".to_string(), passphrase: "not-a-real-secret".to_string() },
            ),
            (
                serde_json::json!({"SetFtpsPassword": {"host": "files.example.test", "username": "alice", "password": "not-a-real-secret"}}),
                DownloadsRequest::SetFtpsPassword { host: "files.example.test".to_string(), port: 21, username: "alice".to_string(), password: "not-a-real-secret".to_string() },
            ),
        ] {
            let raw = serde_json::json!({"request_id": 1, "message": message});
            let bytes = serde_json::to_vec(&raw).unwrap();
            let envelope: Envelope<DownloadsRequest> = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(envelope.request_id, Some(1));
            assert_eq!(envelope.message, expected);
        }
    }

    #[test]
    fn every_reply_round_trips_including_a_fully_populated_transfer() {
        for (i, reply) in every_reply().into_iter().enumerate() {
            for request_id in [None, Some(i as u64 + 1)] {
                let (mut a, mut b) = UnixStream::pair().unwrap();
                write_downloads_reply(&mut a, request_id, &reply).unwrap();
                assert_eq!(read_downloads_reply(&mut b).unwrap(), (request_id, reply.clone()));
            }
        }
    }

    #[test]
    fn transfer_info_tolerates_missing_fields_from_an_older_peer() {
        // Adding a `#[serde(default)]` field must not need a protocol
        // version bump (the same rule `blueice_ipc::PROTOCOL_VERSION`
        // states), so a record missing everything but its identity still
        // has to parse.
        let info: TransferInfo = serde_json::from_str(r#"{"id":7,"url":"http://x/y"}"#).unwrap();
        assert_eq!(info.id, 7);
        assert_eq!(info.url, "http://x/y");
        assert_eq!(info.state, TransferState::Queued);
        assert_eq!(info.mode, TransferMode::Undetermined);
        assert_eq!(info.completed_bytes, 0);
        assert!(info.segments.is_empty() && info.events.is_empty());
        assert_eq!(info.total_bytes, None);
    }

    #[test]
    fn transfer_info_ignores_fields_it_does_not_know_about() {
        let info: TransferInfo = serde_json::from_str(r#"{"id":7,"url":"http://x/y","added_in_a_newer_version":[1,2,3]}"#).unwrap();
        assert_eq!(info.id, 7);
    }

    #[test]
    fn an_unrecognized_request_variant_fails_soft_and_keeps_the_request_id() {
        for payload in [r#"{"request_id":5,"message":"Frobnicate"}"#, r#"{"request_id":5,"message":{"Frobnicate":{"x":1}}}"#] {
            let mut buf = Vec::new();
            crate::write_framed(&mut buf, &serde_json::from_str::<serde_json::Value>(payload).unwrap()).unwrap();
            let (request_id, req) = read_downloads_request(&mut std::io::Cursor::new(buf)).unwrap();
            assert_eq!(request_id, Some(5));
            assert_eq!(req, DownloadsRequest::Unknown);
        }
    }

    #[test]
    fn an_unrecognized_reply_variant_fails_soft_and_keeps_the_request_id() {
        for payload in [r#"{"request_id":6,"message":"Frobnicate"}"#, r#"{"request_id":6,"message":{"Frobnicate":[1]}}"#] {
            let mut buf = Vec::new();
            crate::write_framed(&mut buf, &serde_json::from_str::<serde_json::Value>(payload).unwrap()).unwrap();
            let (request_id, reply) = read_downloads_reply(&mut std::io::Cursor::new(buf)).unwrap();
            assert_eq!(request_id, Some(6));
            assert_eq!(reply, DownloadsReply::Unknown);
        }
    }

    #[test]
    fn an_unrecognized_enum_value_inside_a_transfer_falls_back_to_unknown() {
        // A newer downloads process may add a state, a mode, or an error
        // code; an older reader must degrade to `Unknown` for that one
        // field rather than lose the whole record.
        let info: TransferInfo = serde_json::from_str(
            r#"{"id":1,"url":"u","state":"teleporting","mode":{"kind":"carrier_pigeon"},"segments":[{"start":0,"end":1,"completed":0,"state":"levitating"}]}"#,
        )
        .unwrap();
        assert_eq!(info.state, TransferState::Unknown);
        assert_eq!(info.mode, TransferMode::Unknown);
        assert_eq!(info.segments[0].state, SegmentState::Unknown);

        let code: ErrorCode = serde_json::from_str(r#""out_of_cheese""#).unwrap();
        assert_eq!(code, ErrorCode::Unknown);
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        let bad_payload = b"not json";
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        assert!(read_downloads_request(&mut std::io::Cursor::new(buf.clone())).is_err());
        assert!(read_downloads_reply(&mut std::io::Cursor::new(buf)).is_err());
    }

    #[test]
    fn request_id_is_left_off_the_wire_when_none() {
        let mut buf = Vec::new();
        write_downloads_request(&mut buf, None, &DownloadsRequest::Subscribe).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&buf[4..]).unwrap();
        assert!(json.get("request_id").is_none(), "{json}");
    }

    #[test]
    fn enum_values_are_snake_case_on_the_wire_and_in_display() {
        // These strings are what an AI agent reads in MCP results and
        // passes back as a `state` filter, so they are part of the
        // contract, not an implementation detail.
        for (state, name) in [
            (TransferState::Queued, "queued"),
            (TransferState::AwaitingClearance, "awaiting_clearance"),
            (TransferState::Active, "active"),
            (TransferState::Paused, "paused"),
            (TransferState::Completed, "completed"),
            (TransferState::Failed, "failed"),
            (TransferState::Cancelled, "cancelled"),
            (TransferState::Blocked, "blocked"),
        ] {
            assert_eq!(serde_json::to_string(&state).unwrap(), format!("\"{name}\""));
            assert_eq!(state.to_string(), name);
        }
        assert_eq!(serde_json::to_value(TransferMode::Segmented).unwrap(), serde_json::json!({"kind": "segmented"}));
        assert_eq!(
            serde_json::to_value(TransferMode::SingleStream { reason: SingleStreamReason::ServerIgnoresRange }).unwrap(),
            serde_json::json!({"kind": "single_stream", "reason": "server_ignores_range"})
        );
    }

    #[test]
    fn only_completed_failed_cancelled_and_blocked_are_terminal() {
        for (state, terminal) in [
            (TransferState::Queued, false),
            (TransferState::AwaitingClearance, false),
            (TransferState::Active, false),
            (TransferState::Paused, false),
            (TransferState::Completed, true),
            (TransferState::Failed, true),
            (TransferState::Cancelled, true),
            (TransferState::Blocked, true),
            (TransferState::Unknown, false),
        ] {
            assert_eq!(state.is_terminal(), terminal, "{state}");
        }
    }

    #[test]
    fn fraction_complete_is_none_without_a_usable_total_and_clamped_otherwise() {
        let mut info = TransferInfo { total_bytes: Some(1_000), completed_bytes: 400, ..TransferInfo::default() };
        assert_eq!(info.fraction_complete(), Some(0.4));
        info.completed_bytes = 1_500;
        assert_eq!(info.fraction_complete(), Some(1.0));
        info.total_bytes = None;
        assert_eq!(info.fraction_complete(), None);
        info.total_bytes = Some(0);
        assert_eq!(info.fraction_complete(), None);
    }

    #[test]
    fn default_downloads_socket_path_has_its_own_file_name() {
        let path = default_downloads_socket_path();
        assert_eq!(path.file_name().unwrap(), "downloads.sock");
        for other in ["core.sock", "ai-gatekeeper.sock"] {
            assert_ne!(path.file_name().unwrap(), other);
        }
    }

    #[test]
    fn byte_counts_use_binary_units_with_one_decimal() {
        const MIB: u64 = 1024 * 1024;
        for (bytes, text) in [(0, "0 B"), (1, "1 B"), (1023, "1023 B"), (1024, "1.0 KiB"), (1536, "1.5 KiB"), (MIB, "1.0 MiB"), (12 * MIB + MIB / 4, "12.3 MiB"), (5 * 1024 * MIB, "5.0 GiB"), (3 * 1024 * 1024 * MIB, "3.0 TiB")] {
            assert_eq!(format_bytes(bytes), text, "{bytes}");
        }
        assert_eq!(format_bytes(1024 * 1024 - 1), "1.0 MiB", "just under a unit rounds up into it rather than printing 1024.0 KiB");
    }

    #[test]
    fn speeds_are_bytes_per_second() {
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_speed(12 * 1024 * 1024 + 1024 * 256), "12.3 MiB/s");
    }

    #[test]
    fn durations_read_naturally_and_drop_the_finest_unit_once_it_stops_mattering() {
        for (secs, text) in [(0, "0 s"), (59, "59 s"), (60, "1 min"), (80, "1 min 20 s"), (3599, "59 min 59 s"), (3600, "1 h"), (3725, "1 h 2 min"), (86_400, "1 d"), (90_000, "1 d 1 h")] {
            assert_eq!(format_duration(secs), text, "{secs}");
        }
    }

    // ---- DownloadsClient, against a fake server on a real socket pair ----

    fn serve<F: FnOnce(UnixStream) + Send + 'static>(f: F) -> (UnixStream, thread::JoinHandle<()>) {
        let (client, server) = UnixStream::pair().unwrap();
        (client, thread::spawn(move || f(server)))
    }

    fn answer_hello(server: &mut UnixStream) {
        let (id, req) = read_downloads_request(server).unwrap();
        assert_eq!(req, DownloadsRequest::Hello { protocol_version: DOWNLOADS_PROTOCOL_VERSION });
        write_downloads_reply(server, id, &DownloadsReply::Hello { protocol_version: DOWNLOADS_PROTOCOL_VERSION }).unwrap();
    }

    #[test]
    fn connect_performs_the_hello_handshake() {
        let (stream, server) = serve(|mut s| answer_hello(&mut s));
        DownloadsClient::connect(stream).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn connect_surfaces_a_rejected_handshake_as_a_remote_error() {
        let (stream, server) = serve(|mut s| {
            let (id, _) = read_downloads_request(&mut s).unwrap();
            write_downloads_reply(&mut s, id, &DownloadsReply::Error { code: ErrorCode::UnsupportedVersion, message: "speak v1".to_string() }).unwrap();
        });
        match DownloadsClient::connect(stream) {
            Err(ClientError::Remote { code: ErrorCode::UnsupportedVersion, message }) => assert_eq!(message, "speak v1"),
            other => panic!("{other:?}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn connect_rejects_a_peer_speaking_a_different_protocol_version() {
        let (stream, server) = serve(|mut s| {
            let (id, _) = read_downloads_request(&mut s).unwrap();
            write_downloads_reply(&mut s, id, &DownloadsReply::Hello { protocol_version: DOWNLOADS_PROTOCOL_VERSION + 1 }).unwrap();
        });
        assert!(matches!(DownloadsClient::connect(stream), Err(ClientError::Unexpected(_))));
        server.join().unwrap();
    }

    #[test]
    fn each_call_gets_the_reply_addressed_to_it() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            let (id, req) = read_downloads_request(&mut s).unwrap();
            assert_eq!(req, DownloadsRequest::Start { url: "https://example.com/a.bin".to_string(), dest: Some("a.bin".to_string()), overwrite: true });
            assert!(id.is_some(), "a client must tag requests so replies can be matched");
            write_downloads_reply(&mut s, id, &DownloadsReply::Started(sample_info(4))).unwrap();

            let (id, req) = read_downloads_request(&mut s).unwrap();
            assert_eq!(req, DownloadsRequest::List { state: Some(TransferState::Active) });
            write_downloads_reply(&mut s, id, &DownloadsReply::Transfers(vec![sample_info(4)])).unwrap();

            let (id, req) = read_downloads_request(&mut s).unwrap();
            assert_eq!(req, DownloadsRequest::Get { id: 4 });
            write_downloads_reply(&mut s, id, &DownloadsReply::Transfer(sample_info(4))).unwrap();
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        assert_eq!(client.start("https://example.com/a.bin", Some("a.bin"), true).unwrap().id, 4);
        assert_eq!(client.list(Some(TransferState::Active)).unwrap().len(), 1);
        assert_eq!(client.get(4).unwrap(), sample_info(4));
        server.join().unwrap();
    }

    #[test]
    fn credential_client_calls_send_the_secret_only_in_the_request() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            let (id, request) = read_downloads_request(&mut s).unwrap();
            assert_eq!(request, DownloadsRequest::SetSftpPassword { host: "files.example.test".to_string(), port: 2222, username: "alice".to_string(), password: "not-a-real-secret".to_string() });
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
            let (id, request) = read_downloads_request(&mut s).unwrap();
            assert_eq!(request, DownloadsRequest::RemoveSftpPassword { host: "files.example.test".to_string(), port: 2222, username: "alice".to_string() });
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
            let (id, request) = read_downloads_request(&mut s).unwrap();
            assert_eq!(request, DownloadsRequest::SetSftpPrivateKeyPassphrase { host: "files.example.test".to_string(), port: 2222, username: "alice".to_string(), passphrase: "not-a-real-secret".to_string() });
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
            let (id, request) = read_downloads_request(&mut s).unwrap();
            assert_eq!(request, DownloadsRequest::RemoveSftpPrivateKeyPassphrase { host: "files.example.test".to_string(), port: 2222, username: "alice".to_string() });
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
            let (id, request) = read_downloads_request(&mut s).unwrap();
            assert_eq!(request, DownloadsRequest::SetFtpsPassword { host: "files.example.test".to_string(), port: 2121, username: "alice".to_string(), password: "not-a-real-secret".to_string() });
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
            let (id, request) = read_downloads_request(&mut s).unwrap();
            assert_eq!(request, DownloadsRequest::RemoveFtpsPassword { host: "files.example.test".to_string(), port: 2121, username: "alice".to_string() });
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        client.set_sftp_password("files.example.test", 2222, "alice", "not-a-real-secret").unwrap();
        client.remove_sftp_password("files.example.test", 2222, "alice").unwrap();
        client.set_sftp_private_key_passphrase("files.example.test", 2222, "alice", "not-a-real-secret").unwrap();
        client.remove_sftp_private_key_passphrase("files.example.test", 2222, "alice").unwrap();
        client.set_ftps_password("files.example.test", 2121, "alice", "not-a-real-secret").unwrap();
        client.remove_ftps_password("files.example.test", 2121, "alice").unwrap();
        server.join().unwrap();
    }

    #[test]
    fn pause_resume_cancel_return_the_updated_transfer_and_remove_returns_unit() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            for (expected, state) in [
                (DownloadsRequest::Pause { id: 2 }, TransferState::Paused),
                (DownloadsRequest::Resume { id: 2 }, TransferState::AwaitingClearance),
                (DownloadsRequest::Cancel { id: 2 }, TransferState::Cancelled),
            ] {
                let (id, req) = read_downloads_request(&mut s).unwrap();
                assert_eq!(req, expected);
                let mut info = sample_info(2);
                info.state = state;
                write_downloads_reply(&mut s, id, &DownloadsReply::Transfer(info)).unwrap();
            }
            let (id, req) = read_downloads_request(&mut s).unwrap();
            assert_eq!(req, DownloadsRequest::Remove { id: 2 });
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        assert_eq!(client.pause(2).unwrap().state, TransferState::Paused);
        assert_eq!(client.resume(2).unwrap().state, TransferState::AwaitingClearance);
        assert_eq!(client.cancel(2).unwrap().state, TransferState::Cancelled);
        client.remove(2).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn a_remote_error_reply_becomes_a_typed_client_error() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            let (id, _) = read_downloads_request(&mut s).unwrap();
            write_downloads_reply(&mut s, id, &DownloadsReply::Error { code: ErrorCode::NotFound, message: "no transfer 9".to_string() }).unwrap();
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        match client.get(9) {
            Err(ClientError::Remote { code: ErrorCode::NotFound, message }) => assert_eq!(message, "no transfer 9"),
            other => panic!("{other:?}"),
        }
        server.join().unwrap();
    }

    #[test]
    fn a_reply_of_the_wrong_shape_is_reported_not_misinterpreted() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            let (id, _) = read_downloads_request(&mut s).unwrap();
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        assert!(matches!(client.get(1), Err(ClientError::Unexpected(_))));
        server.join().unwrap();
    }

    #[test]
    fn pushed_updates_that_interleave_with_a_reply_are_kept_not_lost() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            let (id, _) = read_downloads_request(&mut s).unwrap();
            // Two pushes (no request id), a reply to some other request
            // (e.g. a broker fanning another client's traffic out to us),
            // and only then the reply this call is waiting for.
            write_downloads_reply(&mut s, None, &DownloadsReply::Updated(sample_info(1))).unwrap();
            write_downloads_reply(&mut s, None, &DownloadsReply::Updated(sample_info(2))).unwrap();
            write_downloads_reply(&mut s, Some(9_999), &DownloadsReply::Ok).unwrap();
            write_downloads_reply(&mut s, id, &DownloadsReply::Transfer(sample_info(3))).unwrap();
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        assert_eq!(client.get(3).unwrap().id, 3);
        let pushed: Vec<u64> = client.take_buffered_updates().iter().map(|t| t.id).collect();
        assert_eq!(pushed, vec![1, 2]);
        assert!(client.take_buffered_updates().is_empty(), "taking the buffer must drain it");
        server.join().unwrap();
    }

    #[test]
    fn subscribe_then_next_update_delivers_pushes_in_order() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            let (id, req) = read_downloads_request(&mut s).unwrap();
            assert_eq!(req, DownloadsRequest::Subscribe);
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
            for n in 1..=3 {
                let mut info = sample_info(1);
                info.completed_bytes = n * 100;
                write_downloads_reply(&mut s, None, &DownloadsReply::Updated(info)).unwrap();
            }
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        client.subscribe().unwrap();
        let seen: Vec<u64> = (0..3).map(|_| client.next_update().unwrap().completed_bytes).collect();
        assert_eq!(seen, vec![100, 200, 300]);
        server.join().unwrap();
    }

    #[test]
    fn a_stalled_update_buffer_is_bounded() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            let (id, _) = read_downloads_request(&mut s).unwrap();
            for n in 0..(MAX_BUFFERED_UPDATES as u64 + 10) {
                write_downloads_reply(&mut s, None, &DownloadsReply::Updated(sample_info(n))).unwrap();
            }
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        client.remove(1).unwrap();
        let buffered = client.take_buffered_updates();
        assert_eq!(buffered.len(), MAX_BUFFERED_UPDATES);
        // The oldest are dropped: the newest state of each transfer is the one worth keeping.
        assert_eq!(buffered.last().unwrap().id, MAX_BUFFERED_UPDATES as u64 + 9);
        assert_eq!(buffered.first().unwrap().id, 10);
        server.join().unwrap();
    }

    #[test]
    fn next_update_returns_a_buffered_push_before_touching_the_stream() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            let (id, _) = read_downloads_request(&mut s).unwrap();
            write_downloads_reply(&mut s, None, &DownloadsReply::Updated(sample_info(1))).unwrap();
            write_downloads_reply(&mut s, id, &DownloadsReply::Ok).unwrap();
            // ...and the server is gone: a second read would be an error.
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        client.remove(5).unwrap();
        server.join().unwrap();
        assert_eq!(client.next_update().unwrap().id, 1);
        assert!(client.next_update().is_err(), "with the buffer empty and the peer gone, next_update must read the stream and fail");
    }

    #[test]
    fn next_update_skips_replies_that_are_not_updates() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            write_downloads_reply(&mut s, Some(77), &DownloadsReply::Ok).unwrap();
            write_downloads_reply(&mut s, None, &DownloadsReply::Unknown).unwrap();
            write_downloads_reply(&mut s, None, &DownloadsReply::Updated(sample_info(8))).unwrap();
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        assert_eq!(client.next_update().unwrap().id, 8);
        server.join().unwrap();
    }

    #[test]
    fn start_parses_without_its_optional_fields() {
        // A client that predates `overwrite`, or simply doesn't care about
        // `dest`, sends neither -- that must still be a valid `Start`.
        let mut buf = Vec::new();
        let raw = serde_json::json!({"request_id": 1, "message": {"Start": {"url": "https://example.com/a"}}});
        crate::write_framed(&mut buf, &raw).unwrap();
        let (_, req) = read_downloads_request(&mut std::io::Cursor::new(buf)).unwrap();
        assert_eq!(req, DownloadsRequest::Start { url: "https://example.com/a".to_string(), dest: None, overwrite: false });

        let mut buf = Vec::new();
        crate::write_framed(&mut buf, &serde_json::json!({"message": {"List": {}}})).unwrap();
        let (_, req) = read_downloads_request(&mut std::io::Cursor::new(buf)).unwrap();
        assert_eq!(req, DownloadsRequest::List { state: None });
    }

    #[test]
    fn a_server_that_hangs_up_is_an_io_error() {
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            let _ = read_downloads_request(&mut s).unwrap();
            // drop the connection without replying
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        assert!(matches!(client.get(1), Err(ClientError::Io(_))));
        server.join().unwrap();
    }

    #[test]
    fn every_error_code_has_a_stable_name_on_the_wire_and_in_display() {
        for (code, name) in [
            (ErrorCode::NotFound, "not_found"),
            (ErrorCode::InvalidRequest, "invalid_request"),
            (ErrorCode::InvalidState, "invalid_state"),
            (ErrorCode::UnsupportedVersion, "unsupported_version"),
            (ErrorCode::Internal, "internal"),
            (ErrorCode::Unknown, "unknown"),
        ] {
            assert_eq!(code.as_str(), name);
            assert_eq!(code.to_string(), name);
            assert_eq!(serde_json::to_string(&code).unwrap(), format!("\"{name}\""));
        }
    }

    #[test]
    fn a_reply_of_the_wrong_shape_is_reported_for_every_call_that_expects_a_particular_one() {
        // A server answering `Ok` to a `List`, and a `Transfers` to a `Remove` and a
        // `Subscribe`, is not something to guess a meaning for.
        let (stream, server) = serve(|mut s| {
            answer_hello(&mut s);
            for wrong in [DownloadsReply::Ok, DownloadsReply::Transfers(Vec::new()), DownloadsReply::Transfers(Vec::new())] {
                let (id, _) = read_downloads_request(&mut s).unwrap();
                write_downloads_reply(&mut s, id, &wrong).unwrap();
            }
        });
        let mut client = DownloadsClient::connect(stream).unwrap();
        assert!(matches!(client.list(None), Err(ClientError::Unexpected(_))));
        assert!(matches!(client.remove(1), Err(ClientError::Unexpected(_))));
        assert!(matches!(client.subscribe(), Err(ClientError::Unexpected(_))));
        server.join().unwrap();
    }

    #[test]
    fn client_error_messages_are_human_readable() {
        assert_eq!(ClientError::Remote { code: ErrorCode::NotFound, message: "no transfer 9".to_string() }.to_string(), "downloads process refused the request (not_found): no transfer 9");
        assert_eq!(ClientError::Unexpected("x".to_string()).to_string(), "unexpected reply from the downloads process: x");
        assert!(ClientError::Io(std::io::Error::other("boom")).to_string().contains("boom"));
    }
}
