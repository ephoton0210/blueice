// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The gatekeeper review a download must clear before it may start --
//! `phase-10-download-manager/PLAN.md`'s "Gatekeeper integration" and
//! `phase-7-local-ai/PLAN.md`'s typestate/capability-token decision,
//! applied to the one component both reference engines got weakest.
//!
//! Two stages, chained by *types* rather than by convention:
//!
//! 1. [`Reviewer::review_url`] -> [`UrlCleared`], before any network I/O.
//!    [`probe`](crate::download::probe::probe) requires one.
//! 2. [`Reviewer::review_download`] -> [`DownloadClearance`], after the
//!    probe has made the file's name, type, and size known.
//!    [`Transfer::begin`](crate::download::transfer::Transfer::begin)
//!    requires one.
//!
//! Both tokens have private fields, no public constructor, and are not
//! `Clone`, so the only way to hold one is for the gatekeeper to have
//! actually said yes; skipping a stage is a compile error, not a
//! review-time miss:
//!
//! ```compile_fail
//! fn needs_clone<T: Clone>() {}
//! needs_clone::<blueice_net::download::clearance::DownloadClearance>();
//! ```
//!
//! ```compile_fail
//! use blueice_net::download::clearance::UrlCleared;
//! let _ = UrlCleared { url: "https://example.test/".to_string() };
//! ```
//!
//! [`Probe`](crate::download::probe::Probe) follows the same rule: callers
//! can inspect it through accessors, but cannot construct or clone one to
//! alter what the download-stage review examined.
//!
//! ```compile_fail
//! fn needs_clone<T: Clone>() {}
//! needs_clone::<blueice_net::download::probe::Probe>();
//! ```
//!
//! **Fail-closed**: a gatekeeper that is unreachable, doesn't answer in
//! time, hangs up, or replies with garbage yields [`Blocked`], never a
//! token -- both reference engines treat a slow or missing check as
//! "safe," and this design deliberately does not.

use crate::download::probe::Probe;
use blueice_ipc::gatekeeper::{
    GatekeeperReply, GatekeeperRequest, read_gatekeeper_reply, write_gatekeeper_request,
};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// A review that did not end in clearance, with the reason and category
/// the gatekeeper gave (or, for a gatekeeper that couldn't answer,
/// `gatekeeper-unavailable`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
    pub reason: String,
    pub category: String,
}

impl Blocked {
    fn unavailable(reason: &str) -> Self {
        Blocked {
            reason: reason.to_string(),
            category: "gatekeeper-unavailable".to_string(),
        }
    }
}

/// Proof that the URL stage cleared `url`.
#[derive(Debug)]
pub struct UrlCleared {
    url: String,
}

impl UrlCleared {
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// Proof that both stages cleared a download's requested and final fetched
/// URLs, with this file
/// name, content type, and size. [`Transfer::begin`] checks all four
/// against what it is actually asked to download, so a token for one
/// file can't be spent on another.
///
/// [`Transfer::begin`]: crate::download::transfer::Transfer::begin
#[derive(Debug)]
pub struct DownloadClearance {
    requested_url: String,
    final_url: String,
    file_name: String,
    content_type: Option<String>,
    total_bytes: Option<u64>,
}

impl DownloadClearance {
    pub(crate) fn requested_url(&self) -> &str {
        &self.requested_url
    }

    pub(crate) fn final_url(&self) -> &str {
        &self.final_url
    }

    pub(crate) fn file_name(&self) -> &str {
        &self.file_name
    }

    pub(crate) fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub(crate) fn total_bytes(&self) -> Option<u64> {
        self.total_bytes
    }
}

/// The gatekeeper client: one short-lived connection per check, like
/// `blueice-engine`'s navigation gate.
#[derive(Debug, Clone)]
pub struct Reviewer {
    socket: PathBuf,
    timeout: Duration,
}

impl Reviewer {
    pub fn new(socket: impl AsRef<Path>) -> Self {
        Reviewer {
            socket: socket.as_ref().to_path_buf(),
            timeout: Duration::from_secs(10),
        }
    }

    /// How long a check may take before it counts as a rejection.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The URL stage: `CheckUrl`, before any network I/O.
    pub fn review_url(&self, url: &str) -> Result<UrlCleared, Blocked> {
        self.ask(&GatekeeperRequest::CheckUrl {
            url: url.to_string(),
        })?;
        Ok(UrlCleared {
            url: url.to_string(),
        })
    }

    /// The download stage: `CheckDownload`, once the probe has made the
    /// file's name, type, and size known. It reviews the probe's *final*
    /// URL -- where a redirect actually led -- since that is what the
    /// bytes come from. `cleared` must be for the same URL the probe was
    /// of; anything else is refused without asking.
    pub fn review_download(
        &self,
        cleared: UrlCleared,
        probe: &Probe,
        file_name: &str,
    ) -> Result<DownloadClearance, Blocked> {
        if cleared.url != probe.url {
            return Err(Blocked {
                reason: "the probe is of a different URL than the one that was cleared".to_string(),
                category: "clearance-mismatch".to_string(),
            });
        }
        self.ask(&GatekeeperRequest::CheckDownload {
            url: probe.final_url.clone(),
            file_name: file_name.to_string(),
            content_type: probe.content_type.clone(),
            total_bytes: probe.total,
        })?;
        Ok(DownloadClearance {
            requested_url: probe.url.clone(),
            final_url: probe.final_url.clone(),
            file_name: file_name.to_string(),
            content_type: probe.content_type.clone(),
            total_bytes: probe.total,
        })
    }

    /// One round trip. Runs on a helper thread so the deadline holds even
    /// against a gatekeeper that sends half a frame and stalls (the
    /// framing layer retries a read timeout once some bytes have
    /// arrived); the helper is left to finish on its own if it does.
    fn ask(&self, request: &GatekeeperRequest) -> Result<(), Blocked> {
        let (socket, request, timeout) = (self.socket.clone(), request.clone(), self.timeout);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(exchange(&socket, &request, timeout));
        });
        match rx.recv_timeout(timeout) {
            Ok(Ok(GatekeeperReply::Cleared)) => Ok(()),
            Ok(Ok(GatekeeperReply::Rejected { reason, category })) => {
                Err(Blocked { reason, category })
            }
            Ok(Err(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                Err(Blocked::unavailable(
                    "the gatekeeper did not answer in time",
                ))
            }
            Ok(Err(_)) => Err(Blocked::unavailable("the gatekeeper is unreachable")),
            Err(_) => Err(Blocked::unavailable(
                "the gatekeeper did not answer in time",
            )),
        }
    }
}

fn exchange(
    socket: &Path,
    request: &GatekeeperRequest,
    timeout: Duration,
) -> io::Result<GatekeeperReply> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    write_gatekeeper_request(&mut stream, request)?;
    read_gatekeeper_reply(&mut stream)
}
