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
mod navigation;
mod navigation_transport;
pub use navigation::{FormEncoding, NavigationRequest, MAX_FORM_BODY_BYTES};
pub use navigation_transport::ResponseCancellation;

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

/// An original navigation response whose bytes belong to a download. The body
/// is owned and unread: consuming it never issues a probe or another request.
pub struct FetchedDownload {
    pub final_url: String,
    pub status: u16,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub content_length: Option<u64>,
    body: Option<ureq::Body>,
    cancellation: ResponseCancellation,
}

impl FetchedDownload {
    pub fn cancellation(&self) -> ResponseCancellation {
        self.cancellation.clone()
    }
    pub fn into_reader(mut self) -> impl std::io::Read + Send + 'static {
        DownloadBody {
            reader: self.body.take().expect("owned unread body").into_reader(),
            cancellation: self.cancellation.clone(),
        }
    }
}
impl Drop for FetchedDownload {
    fn drop(&mut self) {
        if self.body.is_some() {
            self.cancellation.cancel();
        }
    }
}
struct DownloadBody {
    reader: ureq::BodyReader<'static>,
    cancellation: ResponseCancellation,
}
impl std::io::Read for DownloadBody {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buffer)
    }
}
impl Drop for DownloadBody {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

fn safe_content_disposition(response: &ureq::http::Response<ureq::Body>) -> Option<String> {
    let value = response
        .headers()
        .get("content-disposition")?
        .to_str()
        .ok()?;
    (value.len() <= 4096 && !value.bytes().any(|byte| byte.is_ascii_control()))
        .then(|| value.to_string())
}

fn response_is_download(content_type: Option<&str>, disposition: Option<&str>) -> bool {
    if let Some(disposition) = disposition {
        let token = disposition.split(';').next().unwrap_or("").trim();
        // RFC 6266: unknown valid disposition types receive attachment handling.
        let valid = !token.is_empty()
            && token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte));
        if valid && !token.eq_ignore_ascii_case("inline") {
            return true;
        }
    }
    let Some(content_type) = content_type else {
        return false; // Preserve the existing unspecified-type HTML path.
    };
    let mime = content_type.split(';').next().unwrap_or("").trim();
    !["text/html", "application/xhtml+xml", "text/plain"]
        .iter()
        .any(|supported| mime.eq_ignore_ascii_case(supported))
}

fn safe_content_type(response: &ureq::http::Response<ureq::Body>) -> Option<String> {
    let value = response.headers().get("content-type")?.to_str().ok()?;
    (value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' '))
    .then(|| value.to_string())
}

/// The result of one HTTP navigation hop with automatic redirects disabled.
/// Core deliberately owns the next-hop decision for browser navigations so it
/// can evaluate gatekeeper and declarative-extension policy before another
/// connection is opened.
pub enum FetchHop {
    /// A non-redirect response body, ready for normal content review.
    Page(FetchedPage),
    /// Download metadata and the unread original response stream.
    Download(FetchedDownload),
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
    canonical_http_origin(url).map(|_| ())
}

/// Returns the canonical tuple origin for a complete HTTP(S) URL without
/// performing a request. This is the shared core policy boundary for callers
/// that need to scope state (such as a page script realm) to the document that
/// navigation accepted, rather than treating an arbitrary input string as an
/// origin identity.
///
/// The network client itself already uses `ureq`'s HTTP URI implementation;
/// relying on that parser here avoids creating a second URL grammar at the
/// page-host boundary. Host names are ASCII-case-normalized and default ports
/// are omitted, which gives the same origin identity to ordinary equivalent
/// HTTP URI spellings.
pub fn canonical_http_origin(url: &str) -> Result<String, FetchError> {
    let uri = url
        .parse::<ureq::http::Uri>()
        .map_err(|error| FetchError::InvalidUrl(format!("malformed URL {url:?}: {error}")))?;
    let scheme = match uri.scheme_str() {
        Some(scheme) if scheme.eq_ignore_ascii_case("http") => "http",
        Some(scheme) if scheme.eq_ignore_ascii_case("https") => "https",
        _ => {
            return Err(FetchError::InvalidUrl(format!(
                "unsupported scheme in {url:?} (only http/https are supported)"
            )));
        }
    };
    let host = uri
        .host()
        .filter(|host| !host.is_empty())
        .ok_or_else(|| FetchError::InvalidUrl(format!("missing HTTP(S) host in {url:?}")))?;
    let port = uri.port_u16();
    let port = match (scheme, port) {
        ("http", Some(80)) | ("https", Some(443)) | (_, None) => String::new(),
        (_, Some(port)) => format!(":{port}"),
    };
    Ok(format!("{scheme}://{}{port}", host.to_ascii_lowercase()))
}

/// Fetches `url` with a plain GET, following redirects, and returns the
/// response body as text. `http://`/`https://` only -- `file://` and
/// anything else is rejected as an unsupported scheme for this
/// reference pass (a local-file loader is a separate, simpler code path
/// that doesn't belong in an HTTP client).
pub fn fetch(url: &str) -> Result<FetchedPage, FetchError> {
    validate_url_scheme(url)?;

    let mut response = ureq::get(url)
        .call()
        .map_err(|e| FetchError::Request(e.to_string()))?;
    // MVP simplification: report the requested URL, not the post-
    // redirect one -- ureq follows redirects transparently but this
    // reference client doesn't yet surface the final effective URL
    // (needed for "the browser's address bar shows where you actually
    // ended up", which has no UI to show it in yet anyway).
    let final_url = url.to_string();
    let status = response.status().as_u16();
    let content_type = safe_content_type(&response);
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| FetchError::Body(e.to_string()))?;
    Ok(FetchedPage {
        final_url,
        body,
        status,
        content_type,
    })
}

/// The resolved, validated HTTP(S) `Location` of a redirect response, or
/// `None` when the response is not a redirect.
fn redirect_target(
    url: &str,
    response: &ureq::http::Response<ureq::Body>,
) -> Result<Option<(String, u16)>, FetchError> {
    let status = response.status().as_u16();
    if !matches!(status, 301 | 302 | 303 | 307 | 308) {
        return Ok(None);
    }
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
    Ok(Some((location.to_string(), status)))
}

/// One hop of a subresource request (a stylesheet today).
pub enum SubresourceHop {
    /// A resolved HTTP(S) redirect target; nothing was requested from it.
    Redirect { location: String, status: u16 },
    /// A non-redirect response, whatever its status or type, read in full.
    Response {
        final_url: String,
        status: u16,
        content_type: Option<String>,
        body: Vec<u8>,
    },
    /// The body is longer than the caller's byte limit; reading stopped there.
    TooLarge,
}

/// Fetches exactly one GET hop for a subresource. Unlike a navigation hop it
/// never treats the response type as a download -- the caller decides whether
/// the type is acceptable -- and it reads at most `max_bytes` of body. Redirects
/// stay off so the caller can review every target before it is requested.
pub fn fetch_subresource_hop(url: &str, max_bytes: usize) -> Result<SubresourceHop, FetchError> {
    validate_url_scheme(url)?;
    let agent = navigation_transport::agent(ResponseCancellation::default());
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| FetchError::Request(error.to_string()))?;
    if let Some((location, status)) = redirect_target(url, &response)? {
        return Ok(SubresourceHop::Redirect { location, status });
    }
    let status = response.status().as_u16();
    let content_type = safe_content_type(&response);
    let limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let body = response.body_mut().with_config().limit(limit).read_to_vec();
    match body {
        Ok(body) if body.len() > max_bytes => Ok(SubresourceHop::TooLarge),
        Ok(body) => Ok(SubresourceHop::Response {
            final_url: url.to_string(),
            status,
            content_type,
            body,
        }),
        Err(ureq::Error::BodyExceedsLimit(_)) => Ok(SubresourceHop::TooLarge),
        Err(error) => Err(FetchError::Body(error.to_string())),
    }
}

/// A request a page's own script makes (`fetch`, `XMLHttpRequest`).
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Headers the caller (the browser itself, not the page) sets, such as
    /// `Origin` and the CORS preflight headers.
    pub trusted_headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// What came back for one hop of a [`ScriptRequest`].
pub enum ScriptHop {
    /// A redirect target, resolved; nothing was requested from it.
    Redirect {
        location: String,
        status: u16,
    },
    Response {
        status: u16,
        status_text: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    TooLarge,
}

/// Request headers a page's script may not set (the browser owns them).
fn forbidden_request_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("sec-")
        || name.starts_with("proxy-")
        || matches!(
            name.as_str(),
            "accept-charset"
                | "accept-encoding"
                | "access-control-request-headers"
                | "access-control-request-method"
                | "connection"
                | "content-length"
                | "cookie"
                | "cookie2"
                | "date"
                | "dnt"
                | "expect"
                | "host"
                | "keep-alive"
                | "origin"
                | "referer"
                | "set-cookie"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
                | "via"
        )
}

/// Executes exactly one hop of a script's request: any of GET, HEAD, POST, PUT,
/// PATCH and DELETE, with the script's headers (minus the forbidden ones) and
/// body, no automatic redirects and at most `max_bytes` of response body.
/// Cookies are not sent; the caller reviews every target before calling.
pub fn fetch_script_request_hop(
    request: &ScriptRequest,
    max_bytes: usize,
) -> Result<ScriptHop, FetchError> {
    validate_url_scheme(&request.url)?;
    let method = request.method.to_ascii_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
    ) {
        return Err(FetchError::Request(format!(
            "method {method} is not allowed"
        )));
    }
    let agent = navigation_transport::agent(ResponseCancellation::default());
    let mut builder = ureq::http::Request::builder()
        .method(method.as_str())
        .uri(request.url.as_str());
    for (name, value) in &request.headers {
        if !forbidden_request_header(name) {
            builder = builder.header(name.as_str(), value.as_str());
        }
    }
    for (name, value) in &request.trusted_headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let built = builder
        .body(request.body.clone())
        .map_err(|error| FetchError::Request(error.to_string()))?;
    let mut response = agent
        .run(built)
        .map_err(|error| FetchError::Request(error.to_string()))?;
    if let Some((location, status)) = redirect_target(&request.url, &response)? {
        return Ok(ScriptHop::Redirect { location, status });
    }
    let status = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str().to_ascii_lowercase();
            // The page never sees cookies it is not entitled to.
            (name != "set-cookie" && name != "set-cookie2")
                .then(|| value.to_str().ok().map(|v| (name, v.to_string())))
                .flatten()
        })
        .collect();
    let limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    match response.body_mut().with_config().limit(limit).read_to_vec() {
        Ok(body) if body.len() > max_bytes => Ok(ScriptHop::TooLarge),
        Ok(body) => Ok(ScriptHop::Response {
            status,
            status_text,
            headers,
            body,
        }),
        Err(ureq::Error::BodyExceedsLimit(_)) => Ok(ScriptHop::TooLarge),
        Err(error) => Err(FetchError::Body(error.to_string())),
    }
}

/// Fetches exactly one HTTP navigation hop, never following a `Location`
/// response automatically. A redirect target is resolved against `url` and
/// must itself be HTTP(S), but no request to it is made here. This preserves a
/// review point between every connection a navigation may open.
pub fn fetch_navigation_hop(url: &str) -> Result<FetchHop, FetchError> {
    fetch_navigation_request_hop(&NavigationRequest::get(url))
}

/// Executes exactly one reviewed GET/POST hop. Automatic redirects stay off;
/// core owns method rewriting, resubmission and per-target policy checks.
pub fn fetch_navigation_request_hop(request: &NavigationRequest) -> Result<FetchHop, FetchError> {
    let url = request.url();
    validate_url_scheme(url)?;
    let cancellation = ResponseCancellation::default();
    let agent = navigation_transport::agent(cancellation.clone());
    let mut response = if let Some(content_type) = request.content_type() {
        agent
            .post(url)
            .header("Content-Type", content_type)
            .send(request.body())
    } else {
        agent.get(url).call()
    }
    .map_err(|error| FetchError::Request(error.to_string()))?;
    if let Some((location, status)) = redirect_target(url, &response)? {
        return Ok(FetchHop::Redirect { location, status });
    }
    let status = response.status().as_u16();
    let content_type = safe_content_type(&response);
    let content_disposition = safe_content_disposition(&response);
    if response_is_download(content_type.as_deref(), content_disposition.as_deref()) {
        let content_length = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        return Ok(FetchHop::Download(FetchedDownload {
            final_url: url.to_string(),
            status,
            content_type,
            content_disposition,
            content_length,
            body: Some(response.into_body()),
            cancellation,
        }));
    }
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
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\n<p>hello</p>\n",
        );
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
        assert!(matches!(
            validate_url_scheme("file:///etc/passwd"),
            Err(FetchError::InvalidUrl(_))
        ));
        assert!(matches!(
            validate_url_scheme("not-a-valid-url"),
            Err(FetchError::InvalidUrl(_))
        ));
    }

    #[test]
    fn canonical_http_origin_normalizes_host_case_and_default_ports() {
        assert_eq!(
            canonical_http_origin("HTTP://EXAMPLE.test:80/a?x=1#section").unwrap(),
            "http://example.test"
        );
        assert_eq!(
            canonical_http_origin("https://Example.test:443/app").unwrap(),
            "https://example.test"
        );
        assert_eq!(
            canonical_http_origin("https://example.test:8443/app").unwrap(),
            "https://example.test:8443"
        );
    }

    #[test]
    fn canonical_http_origin_requires_a_complete_http_url() {
        assert!(matches!(
            canonical_http_origin("https:///missing-host"),
            Err(FetchError::InvalidUrl(_))
        ));
        assert!(matches!(
            canonical_http_origin("about:blank"),
            Err(FetchError::InvalidUrl(_))
        ));
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
        let url =
            serve_once("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let result = fetch(&url);
        assert!(matches!(result, Err(FetchError::Request(_))));
    }

    #[test]
    fn fetch_error_messages_are_human_readable() {
        assert_eq!(
            FetchError::InvalidUrl("x".to_string()).to_string(),
            "invalid URL: x"
        );
        assert_eq!(
            FetchError::Request("x".to_string()).to_string(),
            "request failed: x"
        );
        assert_eq!(
            FetchError::Body("x".to_string()).to_string(),
            "reading response body failed: x"
        );
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
            FetchHop::Page(_) | FetchHop::Download(_) => {
                panic!("a 302 must remain a policy-visible redirect hop")
            }
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

    fn serve_n(responses: Vec<&'static str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn a_subresource_hop_returns_a_css_body_that_a_navigation_would_download() {
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Length: 6\r\nConnection: close\r\n\r\np{a:b}",
        );
        match fetch_subresource_hop(&url, 1024).unwrap() {
            SubresourceHop::Response {
                status,
                content_type,
                body,
                ..
            } => {
                assert_eq!(status, 200);
                assert_eq!(content_type.as_deref(), Some("text/css"));
                assert_eq!(body, b"p{a:b}");
            }
            _ => panic!("expected a response"),
        }
    }

    #[test]
    fn a_subresource_hop_reports_a_redirect_without_following_it() {
        let url = serve_once(
            "HTTP/1.1 302 Found\r\nLocation: /next.css\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        match fetch_subresource_hop(&url, 1024).unwrap() {
            SubresourceHop::Redirect { location, status } => {
                assert_eq!(status, 302);
                assert!(location.ends_with("/next.css"), "{location}");
                assert!(location.starts_with("http://127.0.0.1:"));
            }
            _ => panic!("expected a redirect"),
        }
    }

    #[test]
    fn a_subresource_hop_stops_reading_past_the_byte_limit() {
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Length: 20\r\nConnection: close\r\n\r\n01234567890123456789",
        );
        assert!(matches!(
            fetch_subresource_hop(&url, 10).unwrap(),
            SubresourceHop::TooLarge
        ));
        let exact = serve_n(vec![
            "HTTP/1.1 200 OK\r\nContent-Type: text/css\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
        ]);
        assert!(matches!(
            fetch_subresource_hop(&exact, 10).unwrap(),
            SubresourceHop::Response { .. }
        ));
    }

    #[test]
    fn a_subresource_hop_refuses_a_non_http_url_before_any_request() {
        assert!(fetch_subresource_hop("file:///etc/passwd", 10).is_err());
        assert!(fetch_subresource_hop("data:text/css,p{}", 10).is_err());
    }

    #[test]
    fn a_script_request_carries_method_headers_and_body_and_hides_forbidden_headers() {
        use std::sync::mpsc;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // The head and the body may arrive in separate segments.
            stream
                .set_read_timeout(Some(std::time::Duration::from_millis(300)))
                .unwrap();
            let mut received = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                received.extend_from_slice(&buf[..n]);
                if received.ends_with(b"{\"a\":1}") {
                    break;
                }
            }
            tx.send(String::from_utf8_lossy(&received).to_string())
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 201 Created\r\nX-Reply: yes\r\nSet-Cookie: s=1\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let hop = fetch_script_request_hop(
            &ScriptRequest {
                method: "post".into(),
                url: format!("http://{addr}/api"),
                headers: vec![
                    ("X-Custom".into(), "1".into()),
                    ("Cookie".into(), "secret=1".into()),
                    ("Content-Type".into(), "application/json".into()),
                ],
                trusted_headers: vec![],
                body: b"{\"a\":1}".to_vec(),
            },
            1000,
        )
        .unwrap();
        let sent = rx.recv().unwrap().to_ascii_lowercase();
        assert!(sent.starts_with("post /api"));
        assert!(sent.contains("x-custom: 1") && sent.contains("application/json"));
        assert!(
            !sent.contains("secret=1"),
            "a script cannot set Cookie: {sent}"
        );
        assert!(sent.ends_with("{\"a\":1}"));
        let ScriptHop::Response {
            status,
            status_text,
            headers,
            body,
        } = hop
        else {
            panic!("expected a response")
        };
        assert_eq!(
            (status, status_text.as_str(), body.as_slice()),
            (201, "Created", &b"ok"[..])
        );
        assert!(headers.iter().any(|(k, v)| k == "x-reply" && v == "yes"));
        assert!(
            !headers.iter().any(|(k, _)| k == "set-cookie"),
            "cookies are not exposed"
        );
    }

    #[test]
    fn script_requests_refuse_unknown_methods_and_oversized_bodies() {
        let bad = fetch_script_request_hop(
            &ScriptRequest {
                method: "TRACE".into(),
                url: "http://127.0.0.1:1/".into(),
                headers: vec![],
                trusted_headers: vec![],
                body: vec![],
            },
            10,
        );
        assert!(matches!(bad, Err(FetchError::Request(_))));
        let url = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\n01234567890123456789");
        let hop = fetch_script_request_hop(
            &ScriptRequest {
                method: "GET".into(),
                url,
                headers: vec![],
                trusted_headers: vec![],
                body: vec![],
            },
            5,
        )
        .unwrap();
        assert!(matches!(hop, ScriptHop::TooLarge));
    }
}
