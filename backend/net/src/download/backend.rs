// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Protocol-specific operations used by the protocol-neutral transfer
//! coordinator.  A backend is responsible for proving that a range is the
//! requested range of the probed resource; the coordinator only reads the
//! resulting bytes, schedules workers, and writes them at the right offset.

use crate::download::probe::{ContentRange, Probe, Validator, parse_content_range};
use crate::download::{
    DownloadError, DownloadOptions, MAX_TRANSFER_TEXT_BYTES, bounded_transfer_text, http,
};
use std::io::{self, Read};
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

/// A transfer body that can report protocol-level completion after its bytes
/// have been copied. HTTP and SFTP have nothing extra to do, but FTP's data
/// socket must consume its final control-channel reply before a segment is
/// counted as complete.
pub trait FinishableRead: Read + Send {
    fn finish(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The bytes returned by a transfer backend. The stream begins at
/// [`ByteRange::start`] when a range was requested. It is owned and `Send`
/// so one coordinator worker can consume it without sharing protocol state
/// with another worker.
pub type ByteStream = Box<dyn FinishableRead>;

struct PlainRead<R>(R);

impl<R: Read> Read for PlainRead<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.read(buffer)
    }
}

impl<R: Read + Send> FinishableRead for PlainRead<R> {}

/// Adapts a protocol body without a separate completion handshake.
pub(crate) fn plain_stream<R: Read + Send + 'static>(reader: R) -> ByteStream {
    Box::new(PlainRead(reader))
}

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
pub(crate) fn for_url(
    url: &str,
    options: &DownloadOptions,
) -> Result<Arc<dyn TransferBackend>, DownloadError> {
    validate_url(url)?;
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(Arc::new(HttpBackend {
            agent: http::agent(options),
        }))
    } else if url.starts_with("sftp://") {
        Ok(Arc::new(crate::download::sftp::SftpBackend::new(options)))
    } else if url.starts_with("ftp://") || url.starts_with("ftps://") {
        Ok(Arc::new(crate::download::ftp::FtpBackend::new(options)))
    } else {
        Err(DownloadError::InvalidUrl(format!(
            "unsupported scheme in {url:?} (supported: http, https, ftp, ftps, sftp)"
        )))
    }
}

/// Validates a download URL before it is persisted or sent to the
/// gatekeeper. HTTP(S) URLs are parsed here too: userinfo would otherwise be
/// copied into transfer history, MCP output, and the downloads page. SFTP
/// must likewise be parsed because its credentials belong in the OS keychain,
/// never in a URL.
pub fn validate_url(url: &str) -> Result<(), DownloadError> {
    if url.len() > MAX_TRANSFER_TEXT_BYTES {
        return Err(DownloadError::InvalidUrl(format!(
            "a download URL exceeds the {}-byte limit",
            MAX_TRANSFER_TEXT_BYTES
        )));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let uri: ureq::http::Uri = url.parse().map_err(|error| {
            DownloadError::InvalidUrl(format!("invalid HTTP URL {url:?}: {error}"))
        })?;
        let authority = uri
            .authority()
            .ok_or_else(|| DownloadError::InvalidUrl(format!("HTTP URL {url:?} has no host")))?;
        if authority.as_str().contains('@') {
            return Err(DownloadError::InvalidUrl("HTTP(S) URLs must not contain userinfo; use a credential mechanism that does not persist secrets in the URL".to_string()));
        }
        Ok(())
    } else if url.starts_with("sftp://") {
        crate::download::sftp::validate_url(url)
    } else if url.starts_with("ftp://") || url.starts_with("ftps://") {
        crate::download::ftp::validate_url(url)
    } else {
        Err(DownloadError::InvalidUrl(format!(
            "unsupported scheme in {url:?} (supported: http, https, ftp, ftps, sftp)"
        )))
    }
}

struct HttpBackend {
    agent: Agent,
}

impl HttpBackend {
    /// A `206` must be for exactly the requested range of the resource that
    /// was probed.  Keeping HTTP's headers here prevents the coordinator from
    /// acquiring HTTP-only knowledge as FTP and SFTP are added.
    fn check_partial(
        response: &Response<Body>,
        probe: &Probe,
        range: ByteRange,
    ) -> Result<(), DownloadError> {
        let value = http::header(response, "content-range").ok_or_else(|| {
            DownloadError::Protocol("a 206 response with no Content-Range".to_string())
        })?;
        match parse_content_range(&value) {
            Some(ContentRange::Range { start, end, total })
                if start == range.start && end == range.end - 1 =>
            {
                if let (Some(now), Some(then)) = (total, probe.total) {
                    if now != then {
                        return Err(DownloadError::ResourceChanged(format!(
                            "the file's size changed from {then} to {now} bytes"
                        )));
                    }
                }
            }
            _ => {
                let value = bounded_transfer_text(&value);
                return Err(DownloadError::Protocol(format!(
                    "unexpected Content-Range {value:?} in answer to bytes={}-{}",
                    range.start,
                    range.end - 1
                )));
            }
        }
        match probe.validator() {
            Some(Validator::StrongEtag(expected)) => {
                if let Some(got) = http::header(response, "etag").filter(|got| got != &expected) {
                    return Err(DownloadError::ResourceChanged(format!(
                        "the ETag changed from {expected} to {got}"
                    )));
                }
            }
            Some(Validator::LastModified(expected)) => {
                if let Some(got) =
                    http::header(response, "last-modified").filter(|got| got != &expected)
                {
                    return Err(DownloadError::ResourceChanged(format!(
                        "Last-Modified changed from {expected} to {got}"
                    )));
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
        let validator = range
            .and_then(|_| probe.validator())
            .map(|value| value.if_range_value().to_string());
        let response = http::get(
            &self.agent,
            &probe.final_url,
            range.map(|value| (value.start, value.end - 1)),
            validator.as_deref(),
        )?;
        match (range, response.status().as_u16()) {
            (None, 200..=299) => Ok(plain_stream(response.into_body().into_reader())),
            (None, code) => Err(DownloadError::Status(code)),
            (Some(range), 206) => {
                Self::check_partial(&response, probe, range)?;
                Ok(plain_stream(response.into_body().into_reader()))
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
        assert!(for_url("ftp://example.test/file", &options).is_ok());
        assert!(for_url("ftps://alice@example.test/file", &options).is_ok());
    }

    #[test]
    fn download_urls_have_a_persistence_safe_length_limit() {
        let too_long = format!(
            "https://example.test/{}",
            "x".repeat(MAX_TRANSFER_TEXT_BYTES)
        );
        assert!(
            matches!(validate_url(&too_long), Err(DownloadError::InvalidUrl(message)) if message.contains("byte limit"))
        );
    }

    #[test]
    fn an_sftp_password_is_rejected_before_a_transfer_can_record_it() {
        assert!(matches!(
            validate_url("sftp://alice:secret@example.test/file"),
            Err(DownloadError::InvalidUrl(_))
        ));
    }

    #[test]
    fn http_userinfo_is_rejected_before_it_can_enter_history_or_mcp_output() {
        for url in [
            "https://alice:secret@example.test/file",
            "http://token@example.test/file",
        ] {
            assert!(
                matches!(validate_url(url), Err(DownloadError::InvalidUrl(_))),
                "{url}"
            );
        }
        assert!(validate_url("https://example.test/file").is_ok());
    }
}
