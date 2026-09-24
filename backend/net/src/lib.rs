// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTTP fetching for navigation (`phase-4-human-rendering-path/PLAN.md`'s
//! "handle basic navigation: load a URL").
//!
//! Built on `ureq` rather than hand-rolled, same reasoning as
//! `blueice-ipc`'s use of `serde`: HTTP-plus-TLS correctness and
//! security is a solved, security-sensitive problem this project's
//! "written from scratch" identity (`CLAUDE.md`) is about the
//! rendering engine, not about reimplementing a TLS stack. Scope is
//! deliberately minimal for this reference pass: a synchronous GET,
//! `text/html` treated as the body encoding (no charset sniffing --
//! `research/html-parsing.md` already assumes UTF-8 or an explicit
//! declaration for the HTML parser itself), redirects followed
//! automatically (`ureq`'s default) for generic callers. Core navigation uses
//! this crate's one-hop API instead, so it can review each redirect target
//! before connecting. No cookies/cache/auth. Real
//! per-site browsing policy (robots.txt, ToS, rate limits --
//! `BROWSER_CORE_PLAN.md` §5's risk register) is explicitly out of
//! scope for this reference frontend, same as it is for the engine's
//! Phase 7 gatekeeper design.

use std::fmt;
use url::Url;

pub mod download;

#[derive(Debug)]
pub enum FetchError {
    /// The URL string itself isn't well-formed enough to request (no
    /// scheme, unsupported scheme, ...).
    InvalidUrl(String),
    /// The request was sent but failed at the network/HTTP level
    /// (connection refused, TLS failure, timeout, non-2xx status).
    Request(String),
    /// The response body couldn't be read/decoded as text.
    Body(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::InvalidUrl(s) => write!(f, "invalid URL: {s}"),
            FetchError::Request(s) => write!(f, "request failed: {s}"),
            FetchError::Body(s) => write!(f, "reading response body failed: {s}"),
        }
    }
}

impl std::error::Error for FetchError {}

pub struct FetchedPage {
    pub final_url: String,
    pub body: String,
    pub status: u16,
    pub content_type: Option<String>,
}

fn safe_content_type(response: &ureq::http::Response<ureq::Body>) -> Option<String> {
    let value = response.headers().get("content-type")?.to_str().ok()?;
    (value.len() <= 256 && value.bytes().all(|byte| byte.is_ascii_graphic() || byte == b' '))
        .then(|| value.to_string())
}

/// The result of one HTTP navigation hop with automatic redirects disabled.
/// Core deliberately owns the next-hop decision for browser navigations so it
/// can evaluate gatekeeper and declarative-extension policy before another
/// connection is opened.
pub enum FetchHop {
    /// A non-redirect response body, ready for normal content review.
    Page(FetchedPage),
    /// A resolved, validated HTTP(S) target from one redirect response.
    Redirect { location: String, status: u16 },
}

/// Rejects a non-`http(s)` scheme before any network I/O -- split out
/// of [`fetch`] so a caller that needs this specific cheap, synchronous
/// check without actually fetching (`blueice-engine`'s `session.rs`
/// validates a navigation target's scheme synchronously, before
/// spawning a background gatekeeper-check/fetch thread, so a malformed
/// URL still gets an immediate error reply -- see that module's own
/// docs) doesn't have to duplicate the check or its message format.
pub fn validate_url_scheme(url: &str) -> Result<(), FetchError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(FetchError::InvalidUrl(format!("unsupported scheme in {url:?} (only http/https are supported)")));
    }
    Ok(())
}

/// Fetches `url` with a plain GET, following redirects, and returns the
/// response body as text. `http://`/`https://` only -- `file://` and
/// anything else is rejected as an unsupported scheme for this
/// reference pass (a local-file loader is a separate, simpler code path
/// that doesn't belong in an HTTP client).
pub fn fetch(url: &str) -> Result<FetchedPage, FetchError> {
    validate_url_scheme(url)?;

    let mut response = ureq::get(url).call().map_err(|e| FetchError::Request(e.to_string()))?;
    // MVP simplification: report the requested URL, not the post-
    // redirect one -- ureq follows redirects transparently but this
    // reference client doesn't yet surface the final effective URL
    // (needed for "the browser's address bar shows where you actually
    // ended up", which has no UI to show it in yet anyway).
    let final_url = url.to_string();
    let status = response.status().as_u16();
    let content_type = safe_content_type(&response);
    let body = response.body_mut().read_to_string().map_err(|e| FetchError::Body(e.to_string()))?;
    Ok(FetchedPage { final_url, body, status, content_type })
}

/// Fetches exactly one HTTP navigation hop, never following a `Location`
/// response automatically. A redirect target is resolved against `url` and
/// must itself be HTTP(S), but no request to it is made here. This preserves a
/// review point between every connection a navigation may open.
pub fn fetch_navigation_hop(url: &str) -> Result<FetchHop, FetchError> {
    validate_url_scheme(url)?;
    let agent: ureq::Agent = ureq::config::Config::builder()
        .max_redirects(0)
        .build()
        .into();
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| FetchError::Request(error.to_string()))?;
    if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                FetchError::Request("redirect response has no valid Location header".to_string())
            })?;
        let base = Url::parse(url).map_err(|error| {
            FetchError::InvalidUrl(format!("invalid navigation URL {url:?}: {error}"))
        })?;
        let location = base.join(location).map_err(|error| {
            FetchError::InvalidUrl(format!("redirect Location is invalid for {url:?}: {error}"))
        })?;
        validate_url_scheme(location.as_str())?;
        return Ok(FetchHop::Redirect {
            location: location.to_string(),
            status: response.status().as_u16(),
        });
    }
    let status = response.status().as_u16();
    let content_type = safe_content_type(&response);
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| FetchError::Body(error.to_string()))?;
    Ok(FetchHop::Page(FetchedPage {
        final_url: url.to_string(),
        body,
        status,
        content_type,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    /// A minimal, hand-rolled HTTP/1.0 test server -- no mocking crate
    /// needed for "respond with this status/body to any request".
    fn serve_once(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream.write_all(response.as_bytes()).unwrap();
        });
        format!("http://{addr}")
    }

    #[test]
    fn fetches_a_simple_response_body() {
        let url = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n<p>hello</p>\n");
        let page = fetch(&url).unwrap();
        assert_eq!(page.body, "<p>hello</p>\n");
    }

    #[test]
    fn non_http_scheme_is_rejected_before_any_network_call() {
        let result = fetch("file:///etc/passwd");
        assert!(matches!(result, Err(FetchError::InvalidUrl(_))));
    }

    #[test]
    fn validate_url_scheme_accepts_http_and_https_and_rejects_everything_else() {
        assert!(validate_url_scheme("http://example.com").is_ok());
        assert!(validate_url_scheme("https://example.com").is_ok());
        assert!(matches!(validate_url_scheme("file:///etc/passwd"), Err(FetchError::InvalidUrl(_))));
        assert!(matches!(validate_url_scheme("not-a-valid-url"), Err(FetchError::InvalidUrl(_))));
    }

    #[test]
    fn malformed_url_without_scheme_is_rejected() {
        let result = fetch("example.com");
        assert!(matches!(result, Err(FetchError::InvalidUrl(_))));
    }

    #[test]
    fn connection_failure_is_a_request_error_not_a_panic() {
        // nothing listens on this port
        let result = fetch("http://127.0.0.1:1");
        assert!(matches!(result, Err(FetchError::Request(_))));
    }

    #[test]
    fn http_error_status_is_a_request_error() {
        let url = serve_once("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let result = fetch(&url);
        assert!(matches!(result, Err(FetchError::Request(_))));
    }

    #[test]
    fn fetch_error_messages_are_human_readable() {
        assert_eq!(FetchError::InvalidUrl("x".to_string()).to_string(), "invalid URL: x");
        assert_eq!(FetchError::Request("x".to_string()).to_string(), "request failed: x");
        assert_eq!(FetchError::Body("x".to_string()).to_string(), "reading response body failed: x");
    }

    #[test]
    fn navigation_hop_returns_a_resolved_redirect_without_opening_its_target() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            stream
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: /after\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let url = format!("http://{addr}/before");

        match fetch_navigation_hop(&url).unwrap() {
            FetchHop::Redirect { location, status } => {
                assert_eq!(location, format!("http://{addr}/after"));
                assert_eq!(status, 302);
            }
            FetchHop::Page(_) => panic!("a 302 must remain a policy-visible redirect hop"),
        }
    }

    #[test]
    fn navigation_hop_rejects_a_redirect_without_a_location() {
        let url =
            serve_once("HTTP/1.1 302 Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert!(matches!(
            fetch_navigation_hop(&url),
            Err(FetchError::Request(message)) if message.contains("Location")
        ));
    }
}
