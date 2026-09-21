// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What a probe learns about a resource, and how to read the headers it
//! is learned from (`phase-10-download-manager/PLAN.md`'s "Probe: a
//! ranged `GET`, not `HEAD`").

use crate::download::clearance::UrlCleared;
use crate::download::backend;
use crate::download::{http, DownloadError, DownloadOptions};
use ureq::Agent;
use blueice_ipc::downloads::SingleStreamReason;
use ureq::ResponseExt;

/// The result of probing a URL with a one-byte ranged request: what the
/// server actually does, not merely what it advertises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// The URL as requested.
    pub url: String,
    /// After redirects; segment requests go here directly.
    pub final_url: String,
    /// `None` when the server doesn't say.
    pub total: Option<u64>,
    /// Whether the server answered the ranged probe with `206`.
    pub accepts_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    /// The raw `Content-Disposition` header, for [`crate::download::file_name`].
    pub content_disposition: Option<String>,
    /// Whether this backend's revision marker is strong enough to retain a
    /// partial file across a process restart. HTTP validators are; SFTP's
    /// coarse mtime is only a same-run change detector, not a resume proof.
    pub restart_resume_safe: bool,
}

/// What lets a later request confirm it is looking at the same bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validator {
    StrongEtag(String),
    LastModified(String),
}

impl Validator {
    /// The `If-Range` header value for this validator.
    pub fn if_range_value(&self) -> &str {
        match self {
            Validator::StrongEtag(v) | Validator::LastModified(v) => v,
        }
    }
}

/// `W/"..."`: a weak ETag promises semantic equivalence, not identical
/// bytes, so it can't vouch for a byte-range splice.
pub fn is_weak_etag(etag: &str) -> bool {
    etag.trim_start().starts_with("W/")
}

impl Probe {
    /// The strongest validator available: a strong ETag, else
    /// `Last-Modified`, else none (a weak ETag alone doesn't count --
    /// RFC 9110 requires strong comparison for `If-Range`).
    pub fn validator(&self) -> Option<Validator> {
        match &self.etag {
            Some(etag) if !is_weak_etag(etag) => Some(Validator::StrongEtag(etag.clone())),
            _ => self.last_modified.clone().map(Validator::LastModified),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.total == Some(0)
    }

    /// Segmenting needs working ranges and a known, non-zero length.
    pub fn can_segment(&self) -> bool {
        self.accepts_ranges && self.total.is_some_and(|t| t > 0)
    }

    /// Why this resource can't be segmented, or `None` if it can (or is
    /// empty, so there is nothing to stream).
    pub fn single_stream_reason(&self) -> Option<SingleStreamReason> {
        if self.is_empty() || self.can_segment() {
            None
        } else if !self.accepts_ranges {
            Some(SingleStreamReason::ServerIgnoresRange)
        } else {
            Some(SingleStreamReason::UnknownLength)
        }
    }

    /// Whether pausing keeps the bytes already fetched: the resource is
    /// segmentable *and* there is a validator to prove it hasn't changed
    /// when the transfer resumes.
    pub fn resume_safe(&self) -> bool {
        self.can_segment() && self.restart_resume_safe && self.validator().is_some()
    }
}

/// A parsed `Content-Range` response header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentRange {
    /// `bytes start-end/total` (`total` is `None` for `*`); `end` is inclusive.
    Range { start: u64, end: u64, total: Option<u64> },
    /// `bytes */total`: what a `416` reports about the whole resource.
    Unsatisfied { total: Option<u64> },
}

/// Parses `Content-Range`, rejecting anything malformed, inverted, or
/// reaching past its own declared total.
pub fn parse_content_range(value: &str) -> Option<ContentRange> {
    let (unit, rest) = value.trim().split_once(' ')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (range, total) = rest.trim().split_once('/')?;
    let total = match total.trim() {
        "*" => None,
        digits => Some(digits.parse::<u64>().ok()?),
    };
    if range.trim() == "*" {
        return Some(ContentRange::Unsatisfied { total });
    }
    let (start, end) = range.split_once('-')?;
    let (start, end) = (start.trim().parse::<u64>().ok()?, end.trim().parse::<u64>().ok()?);
    if end < start || total.is_some_and(|t| end >= t) {
        return None;
    }
    Some(ContentRange::Range { start, end, total })
}

/// Probes a cleared URL with a `GET` for `bytes=0-0`. A `206` with a
/// `Content-Range` starting at 0 proves ranges really work and gives the
/// total size; a `200` means the server ignores `Range`; a `416` reporting
/// `*/0` is an empty file. Anything else is a failure. Only the headers
/// are read -- the body is dropped unread -- so a server that ignores the
/// range and starts streaming the whole file costs nothing.
///
/// Requires a [`UrlCleared`]: not one byte goes on the network for a URL
/// the gatekeeper hasn't cleared. Makes a single attempt; retrying a
/// transient failure ([`DownloadError::is_retryable`]) is the caller's
/// policy.
pub fn probe(cleared: &UrlCleared, options: &DownloadOptions) -> Result<Probe, DownloadError> {
    backend::for_url(cleared.url(), options)?.probe(cleared.url())
}

/// HTTP's ranged probe.  It is called by the HTTP backend; other backends
/// provide their protocol's equivalent through
/// [`TransferBackend`](crate::download::backend::TransferBackend).
pub(crate) fn probe_http(url: &str, agent: &Agent) -> Result<Probe, DownloadError> {
    let response = http::get(agent, url, Some((0, 0)), None)?;
    let status = response.status().as_u16();
    let header = |name: &str| http::header(&response, name);

    let (total, accepts_ranges) = match status {
        206 => {
            let value = header("content-range").ok_or_else(|| DownloadError::Protocol("a 206 response with no Content-Range".to_string()))?;
            match parse_content_range(&value) {
                Some(ContentRange::Range { start: 0, total, .. }) => (total, true),
                _ => return Err(DownloadError::Protocol(format!("unusable Content-Range {value:?} in answer to bytes=0-0"))),
            }
        }
        200 => (header("content-length").and_then(|v| v.trim().parse().ok()), false),
        416 => match header("content-range").as_deref().and_then(parse_content_range) {
            Some(ContentRange::Unsatisfied { total: Some(0) }) => (Some(0), false),
            _ => return Err(DownloadError::Status(416)),
        },
        other => return Err(DownloadError::Status(other)),
    };

    Ok(Probe {
        url: url.to_string(),
        final_url: response.get_uri().to_string(),
        total,
        accepts_ranges,
        etag: header("etag"),
        last_modified: header("last-modified"),
        content_type: header("content-type"),
        content_disposition: header("content-disposition"),
        restart_resume_safe: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> Probe {
        Probe {
            url: "http://h/f".to_string(),
            final_url: "http://h/f".to_string(),
            total: Some(1_000),
            accepts_ranges: true,
            etag: Some("\"abc\"".to_string()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
            content_type: Some("application/octet-stream".to_string()),
            content_disposition: None,
            restart_resume_safe: true,
        }
    }

    #[test]
    fn a_strong_etag_is_the_preferred_validator() {
        assert_eq!(probe().validator(), Some(Validator::StrongEtag("\"abc\"".to_string())));
    }

    #[test]
    fn a_weak_etag_is_not_a_validator_so_last_modified_is_used_instead() {
        // RFC 9110 requires strong comparison for `If-Range`, and a weak
        // ETag only promises semantic equivalence, not identical bytes.
        let p = Probe { etag: Some("W/\"abc\"".to_string()), ..probe() };
        assert_eq!(p.validator(), Some(Validator::LastModified("Wed, 21 Oct 2015 07:28:00 GMT".to_string())));
    }

    #[test]
    fn a_weak_etag_alone_is_no_validator_at_all() {
        let p = Probe { etag: Some("W/\"abc\"".to_string()), last_modified: None, ..probe() };
        assert_eq!(p.validator(), None);
        assert!(!p.resume_safe());
    }

    #[test]
    fn last_modified_alone_is_a_validator_and_nothing_at_all_is_not() {
        let p = Probe { etag: None, ..probe() };
        assert_eq!(p.validator(), Some(Validator::LastModified("Wed, 21 Oct 2015 07:28:00 GMT".to_string())));
        let p = Probe { etag: None, last_modified: None, ..probe() };
        assert_eq!(p.validator(), None);
        assert!(!p.resume_safe());
    }

    #[test]
    fn the_validator_exposes_the_header_value_to_send_in_if_range() {
        assert_eq!(Validator::StrongEtag("\"abc\"".to_string()).if_range_value(), "\"abc\"");
        assert_eq!(Validator::LastModified("Wed".to_string()).if_range_value(), "Wed");
    }

    #[test]
    fn segmenting_needs_working_ranges_and_a_known_non_zero_length() {
        assert!(probe().can_segment());
        assert!(!Probe { accepts_ranges: false, ..probe() }.can_segment());
        assert!(!Probe { total: None, ..probe() }.can_segment());
        assert!(!Probe { total: Some(0), ..probe() }.can_segment());
    }

    #[test]
    fn the_single_stream_reason_says_why_segmenting_is_off() {
        assert_eq!(probe().single_stream_reason(), None);
        assert_eq!(Probe { accepts_ranges: false, ..probe() }.single_stream_reason(), Some(SingleStreamReason::ServerIgnoresRange));
        assert_eq!(Probe { accepts_ranges: false, total: None, ..probe() }.single_stream_reason(), Some(SingleStreamReason::ServerIgnoresRange));
        assert_eq!(Probe { total: None, ..probe() }.single_stream_reason(), Some(SingleStreamReason::UnknownLength));
        assert_eq!(Probe { total: Some(0), ..probe() }.single_stream_reason(), None, "an empty file needs no stream at all");
    }

    #[test]
    fn an_empty_resource_is_recognized() {
        assert!(Probe { total: Some(0), ..probe() }.is_empty());
        assert!(!probe().is_empty());
        assert!(!Probe { total: None, ..probe() }.is_empty());
    }

    #[test]
    fn resume_is_safe_only_when_segmentable_and_validatable() {
        assert!(probe().resume_safe());
        assert!(!Probe { accepts_ranges: false, ..probe() }.resume_safe());
        assert!(!Probe { etag: None, last_modified: None, ..probe() }.resume_safe());
    }

    #[test]
    fn a_backend_can_use_a_revision_for_live_checks_without_claiming_restart_safe_resume() {
        let sftp_like = Probe { etag: None, last_modified: Some("sftp-mtime:123".to_string()), restart_resume_safe: false, ..probe() };
        assert!(!sftp_like.resume_safe());
    }

    #[test]
    fn content_range_parses_a_served_range() {
        assert_eq!(parse_content_range("bytes 0-0/1000"), Some(ContentRange::Range { start: 0, end: 0, total: Some(1000) }));
        assert_eq!(parse_content_range("bytes 500-999/1000"), Some(ContentRange::Range { start: 500, end: 999, total: Some(1000) }));
        assert_eq!(parse_content_range("  Bytes 5-9/*  "), Some(ContentRange::Range { start: 5, end: 9, total: None }));
    }

    #[test]
    fn content_range_parses_an_unsatisfied_range_report() {
        assert_eq!(parse_content_range("bytes */1000"), Some(ContentRange::Unsatisfied { total: Some(1000) }));
        assert_eq!(parse_content_range("bytes */0"), Some(ContentRange::Unsatisfied { total: Some(0) }));
        assert_eq!(parse_content_range("bytes */*"), Some(ContentRange::Unsatisfied { total: None }));
    }

    #[test]
    fn content_range_rejects_anything_malformed() {
        for bad in ["", "bytes", "bytes abc", "items 0-1/2", "bytes 5-2/10", "bytes 0-1", "bytes 0-x/10", "bytes -1-5/10", "bytes 0-1/x"] {
            assert_eq!(parse_content_range(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_range_that_ends_past_the_declared_total_is_rejected() {
        assert_eq!(parse_content_range("bytes 0-1000/1000"), None);
    }
}
