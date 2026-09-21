// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `probe()` through its public API, against the local test server.

mod common;

use blueice_ipc::downloads::SingleStreamReason;
use blueice_net::download::clearance::Reviewer;
use blueice_net::download::probe::{probe, Probe, Validator};
use blueice_net::download::{DownloadError, DownloadOptions};
use common::{body, FakeGatekeeper, Resource, TestServer};

/// Clears `url` with a fake gatekeeper (a probe needs a `UrlCleared`, and
/// the only way to get one is through review) and probes it.
fn probe_url(url: &str) -> Result<Probe, DownloadError> {
    let gate = FakeGatekeeper::clear_all();
    let cleared = Reviewer::new(&gate.socket).review_url(url).expect("the fake gatekeeper clears everything");
    probe(&cleared, &DownloadOptions::default())
}

#[test]
fn a_range_capable_server_is_probed_as_segmentable() {
    let server = TestServer::start();
    server.serve("/f", Resource::new(body(1_000)));
    let url = server.url("/f");

    let p = probe_url(&url).unwrap();
    assert_eq!(p.url, url);
    assert_eq!(p.final_url, url);
    assert_eq!(p.total, Some(1_000));
    assert!(p.accepts_ranges);
    assert_eq!(p.etag.as_deref(), Some("\"v1\""));
    assert_eq!(p.last_modified.as_deref(), Some("Wed, 21 Oct 2015 07:28:00 GMT"));
    assert_eq!(p.content_type.as_deref(), Some("application/octet-stream"));
    assert!(p.can_segment() && p.resume_safe());
    assert_eq!(p.validator(), Some(Validator::StrongEtag("\"v1\"".to_string())));
}

#[test]
fn the_probe_is_one_ranged_request_that_refuses_content_encoding() {
    let server = TestServer::start();
    server.serve("/f", Resource::new(body(1_000)));
    probe_url(&server.url("/f")).unwrap();
    let log = server.requests();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].range.as_deref(), Some("bytes=0-0"));
    assert_eq!(log[0].status, 206);
    // A gzip'd body would make byte offsets refer to the encoded stream.
    assert_eq!(log[0].accept_encoding.as_deref(), Some("identity"));
}

#[test]
fn a_server_that_ignores_ranges_is_single_stream_with_its_content_length() {
    let server = TestServer::start();
    server.serve("/f", Resource { honor_ranges: false, ..Resource::new(body(1_000)) });
    let p = probe_url(&server.url("/f")).unwrap();
    assert!(!p.accepts_ranges);
    assert_eq!(p.total, Some(1_000));
    assert_eq!(p.single_stream_reason(), Some(SingleStreamReason::ServerIgnoresRange));
    assert!(!p.can_segment() && !p.resume_safe());
}

#[test]
fn a_response_with_no_content_length_has_an_unknown_total() {
    let server = TestServer::start();
    server.serve("/f", Resource { honor_ranges: false, send_content_length: false, ..Resource::new(body(1_000)) });
    let p = probe_url(&server.url("/f")).unwrap();
    assert_eq!(p.total, None);
    assert!(!p.can_segment());
}

#[test]
fn a_redirect_is_followed_and_the_final_url_recorded_next_to_the_requested_one() {
    let server = TestServer::start();
    server.serve("/old", Resource { redirect_to: Some("/new".to_string()), ..Resource::new(body(10)) });
    server.serve("/new", Resource::new(body(500)));
    let p = probe_url(&server.url("/old")).unwrap();
    assert_eq!(p.url, server.url("/old"));
    assert_eq!(p.final_url, server.url("/new"));
    assert_eq!(p.total, Some(500), "the size is the redirect target's, not the redirect response's");
    assert!(p.accepts_ranges, "the Range header must survive the redirect, or ranges look unsupported when they aren't");
}

#[test]
fn content_disposition_and_a_weak_etag_are_kept_verbatim() {
    let server = TestServer::start();
    server.serve(
        "/f",
        Resource { content_disposition: Some("attachment; filename=\"real name.bin\"".to_string()), etag: Some("W/\"weak\"".to_string()), ..Resource::new(body(100)) },
    );
    let p = probe_url(&server.url("/f")).unwrap();
    assert_eq!(p.content_disposition.as_deref(), Some("attachment; filename=\"real name.bin\""));
    assert_eq!(p.etag.as_deref(), Some("W/\"weak\""));
    assert_eq!(p.validator(), Some(Validator::LastModified("Wed, 21 Oct 2015 07:28:00 GMT".to_string())), "a weak ETag is not a validator");
}

#[test]
fn an_empty_file_is_probed_as_empty_whether_the_server_answers_416_or_200() {
    let server = TestServer::start();
    server.serve("/ranged", Resource::new(Vec::new()));
    server.serve("/plain", Resource { honor_ranges: false, ..Resource::new(Vec::new()) });
    for path in ["/ranged", "/plain"] {
        let p = probe_url(&server.url(path)).unwrap();
        assert!(p.is_empty(), "{path}: {p:?}");
        assert!(!p.can_segment());
    }
}

#[test]
fn http_error_statuses_are_reported_with_their_code() {
    let server = TestServer::start();
    server.serve("/busy", Resource { fail_statuses: vec![503], ..Resource::new(body(10)) });
    assert_eq!(probe_url(&server.url("/missing")), Err(DownloadError::Status(404)));
    let busy = probe_url(&server.url("/busy"));
    assert_eq!(busy, Err(DownloadError::Status(503)));
    assert!(busy.unwrap_err().is_retryable());
}

#[test]
fn a_416_that_is_not_about_an_empty_file_is_a_plain_status_error() {
    let server = TestServer::start();
    server.serve("/f", Resource { fail_statuses: vec![416], ..Resource::new(body(10)) });
    assert_eq!(probe_url(&server.url("/f")), Err(DownloadError::Status(416)));
}

#[test]
fn a_url_with_no_host_is_an_invalid_url() {
    let result = probe_url("http://");
    assert!(matches!(result, Err(DownloadError::InvalidUrl(_))), "{result:?}");
}

#[test]
fn a_refused_connection_is_a_network_error() {
    let result = probe_url("http://127.0.0.1:1/f");
    assert!(matches!(result, Err(DownloadError::Network(_))), "{result:?}");
}

#[test]
fn an_unsupported_scheme_is_rejected_before_any_request_is_made() {
    let server = TestServer::start();
    let result = probe_url("file:///tmp/f");
    assert!(matches!(result, Err(DownloadError::InvalidUrl(_))), "{result:?}");
    assert!(server.requests().is_empty());
}

#[test]
fn a_content_range_that_does_not_start_at_zero_is_a_protocol_error() {
    // A server that answers `bytes=0-0` with a range starting elsewhere
    // is not one whose ranges can be trusted to splice.
    let server = TestServer::start();
    server.serve("/f", Resource { content_range_override: Some("bytes 5-5/1000".to_string()), ..Resource::new(body(1_000)) });
    assert!(matches!(probe_url(&server.url("/f")), Err(DownloadError::Protocol(_))));
}

#[test]
fn an_unparsable_content_range_is_a_protocol_error() {
    let server = TestServer::start();
    server.serve("/f", Resource { content_range_override: Some("garbage".to_string()), ..Resource::new(body(1_000)) });
    assert!(matches!(probe_url(&server.url("/f")), Err(DownloadError::Protocol(_))));
}
