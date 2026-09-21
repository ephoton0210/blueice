// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The transfer engine of `phase-10-download-manager/PLAN.md`: probing a
//! URL, carving it into segments, fetching them concurrently into a
//! pre-allocated file, and resuming after a pause or a restart. A plain
//! library -- the `blueice-downloads` process wraps it with queueing,
//! persistence and the wire protocol, but everything hard to get right
//! lives here so it can be test-driven against a local HTTP server.

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

pub mod clearance;
pub mod backend;
pub mod credentials;
pub mod file_name;
mod http;
mod ftp;
pub mod plan;
pub mod probe;
pub mod progress;
mod sftp;
pub mod sidecar;
pub mod transfer;

#[cfg(test)]
pub(crate) mod testutil;

/// The tunables of one transfer. The defaults are this design's own
/// starting values (`phase-10-download-manager/PLAN.md`'s "Defaults"),
/// not Free Download Manager's.
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// Concurrent connections per transfer.
    pub max_connections: usize,
    /// The smallest span worth its own request; a segment is only split
    /// while at least twice this remains.
    pub min_split_bytes: u64,
    /// Consecutive failed attempts a segment gets before the transfer fails.
    pub max_retries: u32,
    pub retry_backoff_base: Duration,
    pub retry_backoff_max: Duration,
    /// How long a segment may go without a byte before it is revoked and
    /// retried (`ureq` has no idle-read timeout of its own).
    pub stall_timeout: Duration,
    /// How long connecting may take.
    pub connect_timeout: Duration,
    /// How long the server may take to start answering a request.
    pub response_timeout: Duration,
    /// The read/write buffer per connection.
    pub buffer_bytes: usize,
    /// How often progress is made durable (sidecar + `fsync`).
    pub checkpoint_interval: Duration,
    /// The coordinator's heartbeat: how often speed is sampled, snapshots
    /// published, and workers topped up.
    pub tick: Duration,
    /// Whether an existing destination may be replaced.
    pub overwrite: bool,
    /// An OpenSSH `known_hosts` file used to verify SFTP server identity.
    /// `None` means the current user's `~/.ssh/known_hosts`; a missing file
    /// is an error, never a trust-on-first-use prompt.
    pub sftp_known_hosts: Option<PathBuf>,
    /// A local private key to try after SSH-agent authentication. Its
    /// passphrase, if any, is held only in the OS credential store.
    pub sftp_private_key: Option<PathBuf>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        DownloadOptions {
            max_connections: 8,
            min_split_bytes: 1024 * 1024,
            max_retries: 5,
            retry_backoff_base: Duration::from_millis(500),
            retry_backoff_max: Duration::from_secs(8),
            stall_timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            response_timeout: Duration::from_secs(30),
            buffer_bytes: 64 * 1024,
            checkpoint_interval: Duration::from_secs(1),
            tick: Duration::from_millis(100),
            overwrite: false,
            sftp_known_hosts: None,
            sftp_private_key: None,
        }
    }
}

impl DownloadOptions {
    /// The wait before retry number `attempt` (1-based): the base,
    /// doubling each time, capped at `retry_backoff_max`.
    pub fn retry_delay(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(20);
        self.retry_backoff_base.saturating_mul(1u32 << exponent).min(self.retry_backoff_max)
    }
}

/// Why a probe, a segment, or a whole transfer failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    InvalidUrl(String),
    /// Connection, DNS, TLS, or timeout failure.
    Network(String),
    Status(u16),
    /// The server sent something this engine can't use (a malformed or
    /// mismatched `Content-Range`, ...).
    Protocol(String),
    /// The body ended early.
    Truncated { got: u64, expected: u64 },
    /// The remote file is no longer the one this transfer started on.
    ResourceChanged(String),
    Io(String),
    DestinationExists(PathBuf),
    /// A clearance token was presented for a different URL or file name
    /// than the transfer it was used to begin.
    ClearanceMismatch(String),
    /// The SSH server was absent from known-hosts or presented another key.
    HostVerification(String),
    /// SSH authentication could not establish the identity requested by the
    /// URL. This deliberately contains no secret.
    Authentication(String),
    /// The operating system credential store could not be used. The message
    /// is safe to show and never includes a secret.
    Credentials(String),
}

impl DownloadError {
    /// Whether trying the same segment again could plausibly succeed:
    /// transient network trouble, a truncated body, or a server-side
    /// `408`/`429`/`5xx`. Everything else (a `404`, a changed resource, a
    /// full disk) won't fix itself.
    pub fn is_retryable(&self) -> bool {
        match self {
            DownloadError::Network(_) | DownloadError::Truncated { .. } => true,
            DownloadError::Status(code) => matches!(code, 408 | 429 | 500..=599),
            _ => false,
        }
    }
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadError::InvalidUrl(what) => write!(f, "invalid URL: {what}"),
            DownloadError::Network(what) => write!(f, "network error: {what}"),
            DownloadError::Status(code) => write!(f, "the server answered with HTTP status {code}"),
            DownloadError::Protocol(what) => write!(f, "unusable server response: {what}"),
            DownloadError::Truncated { got, expected } => write!(f, "the connection ended after {got} of {expected} bytes"),
            DownloadError::ResourceChanged(what) => write!(f, "the remote file changed during the download: {what}"),
            DownloadError::Io(what) => write!(f, "file error: {what}"),
            DownloadError::DestinationExists(path) => write!(f, "the destination {} already exists", path.display()),
            DownloadError::ClearanceMismatch(what) => write!(f, "gatekeeper clearance does not match this transfer: {what}"),
            DownloadError::HostVerification(what) => write!(f, "SSH host verification failed: {what}"),
            DownloadError::Authentication(what) => write!(f, "authentication failed: {what}"),
            DownloadError::Credentials(what) => write!(f, "credential error: {what}"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<io::Error> for DownloadError {
    fn from(e: io::Error) -> Self {
        DownloadError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn default_options_match_the_documented_defaults() {
        let o = DownloadOptions::default();
        assert_eq!(o.max_connections, 8);
        assert_eq!(o.min_split_bytes, 1024 * 1024);
        assert_eq!(o.max_retries, 5);
        assert_eq!(o.retry_backoff_base, Duration::from_millis(500));
        assert_eq!(o.retry_backoff_max, Duration::from_secs(8));
        assert_eq!(o.stall_timeout, Duration::from_secs(30));
        assert_eq!(o.connect_timeout, Duration::from_secs(10));
        assert_eq!(o.response_timeout, Duration::from_secs(30));
        assert_eq!(o.buffer_bytes, 64 * 1024);
        assert_eq!(o.checkpoint_interval, Duration::from_secs(1));
        assert_eq!(o.tick, Duration::from_millis(100));
        assert!(!o.overwrite);
        assert_eq!(o.sftp_known_hosts, None);
        assert_eq!(o.sftp_private_key, None);
    }

    #[test]
    fn retry_delay_doubles_from_the_base_up_to_the_cap() {
        let o = DownloadOptions::default();
        let delays: Vec<u64> = (1..=7).map(|attempt| o.retry_delay(attempt).as_millis() as u64).collect();
        assert_eq!(delays, vec![500, 1_000, 2_000, 4_000, 8_000, 8_000, 8_000]);
    }

    #[test]
    fn retry_delay_treats_attempt_zero_like_the_first_and_never_overflows() {
        let o = DownloadOptions::default();
        assert_eq!(o.retry_delay(0), o.retry_delay(1));
        assert_eq!(o.retry_delay(u32::MAX), Duration::from_secs(8));
    }

    #[test]
    fn only_transient_failures_are_retryable() {
        let retryable = [
            DownloadError::Network("connection reset".to_string()),
            DownloadError::Truncated { got: 5, expected: 10 },
            DownloadError::Status(408),
            DownloadError::Status(429),
            DownloadError::Status(500),
            DownloadError::Status(503),
            DownloadError::Status(599),
        ];
        for e in retryable {
            assert!(e.is_retryable(), "{e:?}");
        }
        let fatal = [
            DownloadError::InvalidUrl("x".to_string()),
            DownloadError::Status(400),
            DownloadError::Status(403),
            DownloadError::Status(404),
            DownloadError::Status(410),
            DownloadError::Status(416),
            DownloadError::Protocol("bad Content-Range".to_string()),
            DownloadError::ResourceChanged("etag".to_string()),
            DownloadError::Io("disk full".to_string()),
            DownloadError::DestinationExists(PathBuf::from("/d/f")),
            DownloadError::ClearanceMismatch("url".to_string()),
            DownloadError::HostVerification("unknown server".to_string()),
            DownloadError::Authentication("no SSH agent identity".to_string()),
            DownloadError::Credentials("keychain is locked".to_string()),
        ];
        for e in fatal {
            assert!(!e.is_retryable(), "{e:?}");
        }
    }

    #[test]
    fn errors_read_as_plain_sentences() {
        assert_eq!(DownloadError::Status(404).to_string(), "the server answered with HTTP status 404");
        assert_eq!(DownloadError::Truncated { got: 5, expected: 10 }.to_string(), "the connection ended after 5 of 10 bytes");
        assert_eq!(DownloadError::DestinationExists(PathBuf::from("/d/f")).to_string(), "the destination /d/f already exists");
        assert!(DownloadError::ResourceChanged("ETag changed".to_string()).to_string().contains("ETag changed"));
        assert!(DownloadError::Io("disk full".to_string()).to_string().contains("disk full"));
        assert!(DownloadError::Network("refused".to_string()).to_string().contains("refused"));
        assert!(DownloadError::Protocol("bad".to_string()).to_string().contains("bad"));
        assert!(DownloadError::InvalidUrl("x".to_string()).to_string().contains("x"));
        assert!(DownloadError::ClearanceMismatch("wrong url".to_string()).to_string().contains("wrong url"));
        assert!(DownloadError::HostVerification("unknown server".to_string()).to_string().contains("unknown server"));
        assert!(DownloadError::Authentication("no SSH agent identity".to_string()).to_string().contains("no SSH agent identity"));
        assert!(DownloadError::Credentials("keychain is locked".to_string()).to_string().contains("keychain is locked"));
    }
}
