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
//! automatically (`ureq`'s default), no cookies/cache/auth. Real
//! per-site browsing policy (robots.txt, ToS, rate limits --
//! `BROWSER_CORE_PLAN.md` §5's risk register) is explicitly out of
//! scope for this reference frontend, same as it is for the engine's
//! Phase 7 gatekeeper design.

use std::fmt;

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
}

/// Fetches `url` with a plain GET, following redirects, and returns the
/// response body as text. `http://`/`https://` only -- `file://` and
/// anything else is rejected as an unsupported scheme for this
/// reference pass (a local-file loader is a separate, simpler code path
/// that doesn't belong in an HTTP client).
pub fn fetch(url: &str) -> Result<FetchedPage, FetchError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(FetchError::InvalidUrl(format!("unsupported scheme in {url:?} (only http/https are supported)")));
    }

    let mut response = ureq::get(url).call().map_err(|e| FetchError::Request(e.to_string()))?;
    // MVP simplification: report the requested URL, not the post-
    // redirect one -- ureq follows redirects transparently but this
    // reference client doesn't yet surface the final effective URL
    // (needed for "the browser's address bar shows where you actually
    // ended up", which has no UI to show it in yet anyway).
    let final_url = url.to_string();
    let body = response.body_mut().read_to_string().map_err(|e| FetchError::Body(e.to_string()))?;
    Ok(FetchedPage { final_url, body })
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
}
