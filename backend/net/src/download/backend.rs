// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Protocol-specific operations used by the protocol-neutral transfer
//! coordinator.  A backend is responsible for proving that a range is the
//! requested range of the probed resource; the coordinator only reads the
//! resulting bytes, schedules workers, and writes them at the right offset.

use crate::download::probe::{parse_content_range, ContentRange, Probe, Validator};
use crate::download::{http, DownloadError, DownloadOptions};
use std::io::Read;
use std::sync::Arc;
use ureq::http::Response;
use ureq::{Agent, Body};

/// An exclusive byte range to fetch from a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn new(start: u64, end: u64) -> Option<Self> {
        (start < end).then_some(ByteRange { start, end })
    }
}

/// The bytes returned by a transfer backend.  The stream begins at
/// [`ByteRange::start`] when a range was requested.  It is owned and `Send`
/// so one coordinator worker can consume it without sharing protocol state
/// with another worker.
pub type ByteStream = Box<dyn Read + Send>;

/// The protocol-specific half of a download.
///
/// This deliberately models only the operations the download manager needs:
/// inspect a file and read either all of it or an exact range.  Directory
/// browsing and upload belong to the future remote-files feature, rather
/// than inflating the download engine with operations it cannot schedule or
/// secure yet.
pub trait TransferBackend: Send + Sync {
    /// Learns the resource metadata needed to plan a download.
    fn probe(&self, url: &str) -> Result<Probe, DownloadError>;

    /// Opens a stream for the entire resource, or precisely `range`.
    fn get(&self, probe: &Probe, range: Option<ByteRange>) -> Result<ByteStream, DownloadError>;
}

/// Selects the backend that owns `url`.  Keep this one dispatch point so a
/// newly supported scheme cannot accidentally bypass the clearance -> probe
/// -> review -> transfer path.
pub(crate) fn for_url(url: &str, options: &DownloadOptions) -> Result<Arc<dyn TransferBackend>, DownloadError> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(Arc::new(HttpBackend { agent: http::agent(options) }))
    } else if url.starts_with("sftp://") {
        Ok(Arc::new(crate::download::sftp::SftpBackend::new(options)))
    } else {
        Err(DownloadError::InvalidUrl(format!("unsupported scheme in {url:?} (supported: http, https, sftp)")))
    }
}

/// Validates a download URL before it is persisted or sent to the
/// gatekeeper. HTTP(S)'s detailed parsing remains in its client, as before;
/// SFTP must be parsed here because an embedded password would otherwise be
/// copied into transfer history and logs before the backend can reject it.
pub fn validate_url(url: &str) -> Result<(), DownloadError> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else if url.starts_with("sftp://") {
        crate::download::sftp::validate_url(url)
    } else {
        Err(DownloadError::InvalidUrl(format!("unsupported scheme in {url:?} (supported: http, https, sftp)")))
    }
}

struct HttpBackend {
    agent: Agent,
}

impl HttpBackend {
    /// A `206` must be for exactly the requested range of the resource that
    /// was probed.  Keeping HTTP's headers here prevents the coordinator from
    /// acquiring HTTP-only knowledge as FTP and SFTP are added.
    fn check_partial(response: &Response<Body>, probe: &Probe, range: ByteRange) -> Result<(), DownloadError> {
        let value = http::header(response, "content-range").ok_or_else(|| DownloadError::Protocol("a 206 response with no Content-Range".to_string()))?;
        match parse_content_range(&value) {
            Some(ContentRange::Range { start, end, total }) if start == range.start && end == range.end - 1 => {
                if let (Some(now), Some(then)) = (total, probe.total) {
                    if now != then {
                        return Err(DownloadError::ResourceChanged(format!("the file's size changed from {then} to {now} bytes")));
                    }
                }
            }
            _ => return Err(DownloadError::Protocol(format!("unexpected Content-Range {value:?} in answer to bytes={}-{}", range.start, range.end - 1))),
        }
        match probe.validator() {
            Some(Validator::StrongEtag(expected)) => {
                if let Some(got) = http::header(response, "etag").filter(|got| got != &expected) {
                    return Err(DownloadError::ResourceChanged(format!("the ETag changed from {expected} to {got}")));
                }
            }
            Some(Validator::LastModified(expected)) => {
                if let Some(got) = http::header(response, "last-modified").filter(|got| got != &expected) {
                    return Err(DownloadError::ResourceChanged(format!("Last-Modified changed from {expected} to {got}")));
                }
            }
            None => {}
        }
        Ok(())
    }
}

impl TransferBackend for HttpBackend {
    fn probe(&self, url: &str) -> Result<Probe, DownloadError> {
        crate::download::probe::probe_http(url, &self.agent)
    }

    fn get(&self, probe: &Probe, range: Option<ByteRange>) -> Result<ByteStream, DownloadError> {
        let validator = range.and_then(|_| probe.validator()).map(|value| value.if_range_value().to_string());
        let response = http::get(&self.agent, &probe.final_url, range.map(|value| (value.start, value.end - 1)), validator.as_deref())?;
        match (range, response.status().as_u16()) {
            (None, 200..=299) => Ok(Box::new(response.into_body().into_reader())),
            (None, code) => Err(DownloadError::Status(code)),
            (Some(range), 206) => {
                Self::check_partial(&response, probe, range)?;
                Ok(Box::new(response.into_body().into_reader()))
            }
            (Some(_), 200) => Err(DownloadError::ResourceChanged("the server answered a Range request with the whole file: the file changed, or ranges stopped working".to_string())),
            (Some(_), 416) => Err(DownloadError::ResourceChanged("the requested range no longer exists: the remote file changed".to_string())),
            (Some(_), code) => Err(DownloadError::Status(code)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_byte_range_is_non_empty_and_uses_an_exclusive_end() {
        assert_eq!(ByteRange::new(4, 9), Some(ByteRange { start: 4, end: 9 }));
        assert_eq!(ByteRange::new(4, 4), None);
        assert_eq!(ByteRange::new(9, 4), None);
    }

    #[test]
    fn only_registered_schemes_select_a_backend() {
        let options = DownloadOptions::default();
        assert!(for_url("https://example.test/file", &options).is_ok());
        assert!(for_url("sftp://alice@example.test/file", &options).is_ok());
        assert!(matches!(for_url("ftp://example.test/file", &options), Err(DownloadError::InvalidUrl(_))));
    }

    #[test]
    fn an_sftp_password_is_rejected_before_a_transfer_can_record_it() {
        assert!(matches!(validate_url("sftp://alice:secret@example.test/file"), Err(DownloadError::InvalidUrl(_))));
    }
}
