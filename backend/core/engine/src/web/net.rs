// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Network requests made by a page's own scripts (`fetch`, `XMLHttpRequest`).
//!
//! A request is a typed job the page hands to the host, never a socket. Every
//! connection it opens -- the request, a CORS preflight, each redirect hop --
//! is first reviewed by the embedder's `review` (the same URL stage and
//! extension block rules a navigation passes), and a cross-origin response is
//! only handed to the page when it opts in with `Access-Control-Allow-Origin`,
//! as in a browser. Cookies and other ambient credentials are never sent, and
//! the response never exposes `Set-Cookie`.

use blueice_net::{fetch_script_request_hop, ScriptHop, ScriptRequest};
use url::Url;

/// Largest response body a script may read.
pub(crate) const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

pub(crate) struct NetJob {
    pub id: u64,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The requesting document's origin.
    pub origin: String,
}

pub(crate) enum NetOutcome {
    Done {
        status: u16,
        status_text: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        url: String,
    },
    Failed(String),
}

fn origin_of(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| parsed.origin().ascii_serialization())
}

/// Headers a cross-origin request may carry without a preflight.
fn safelisted_header(name: &str, value: &str) -> bool {
    match name.to_ascii_lowercase().as_str() {
        "accept" | "accept-language" | "content-language" => true,
        "content-type" => {
            let essence = value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            matches!(
                essence.as_str(),
                "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
            )
        }
        _ => false,
    }
}

fn needs_preflight(method: &str, headers: &[(String, String)]) -> bool {
    !matches!(method, "GET" | "HEAD" | "POST")
        || headers.iter().any(|(k, v)| !safelisted_header(k, v))
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn allows_origin(headers: &[(String, String)], origin: &str) -> bool {
    matches!(header(headers, "access-control-allow-origin"), Some(v) if v == "*" || v == origin)
}

/// Runs a job to completion. Blocking: call it off the session thread.
pub(crate) fn perform(job: &NetJob, review: &dyn Fn(&str) -> bool) -> NetOutcome {
    let fail = |message: &str| NetOutcome::Failed(message.to_string());
    let Some(target_origin) = origin_of(&job.url) else {
        return fail("only http and https requests can be made");
    };
    let method = job.method.to_ascii_uppercase();
    let cross = target_origin != job.origin;

    if cross && needs_preflight(&method, &job.headers) {
        if !review(&job.url) {
            return fail("the request was blocked");
        }
        let names: Vec<String> = job
            .headers
            .iter()
            .map(|(k, _)| k.to_ascii_lowercase())
            .collect();
        let mut trusted = vec![
            ("Origin".to_string(), job.origin.clone()),
            ("Access-Control-Request-Method".to_string(), method.clone()),
        ];
        if !names.is_empty() {
            trusted.push((
                "Access-Control-Request-Headers".to_string(),
                names.join(","),
            ));
        }
        let preflight = fetch_script_request_hop(
            &ScriptRequest {
                method: "OPTIONS".into(),
                url: job.url.clone(),
                headers: vec![],
                trusted_headers: trusted,
                body: vec![],
            },
            64 * 1024,
        );
        let Ok(ScriptHop::Response {
            status, headers, ..
        }) = preflight
        else {
            return fail("CORS preflight request failed");
        };
        let allowed_methods = header(&headers, "access-control-allow-methods").unwrap_or("");
        let allowed_headers = header(&headers, "access-control-allow-headers")
            .unwrap_or("")
            .to_ascii_lowercase();
        let method_ok = matches!(method.as_str(), "GET" | "HEAD" | "POST")
            || allowed_methods
                .split(',')
                .any(|m| m.trim().eq_ignore_ascii_case(&method) || m.trim() == "*");
        let headers_ok = names.iter().all(|n| {
            allowed_headers
                .split(',')
                .any(|a| a.trim() == n || a.trim() == "*")
                || safelisted_header(n, "text/plain")
        });
        if !(200..300).contains(&status)
            || !allows_origin(&headers, &job.origin)
            || !method_ok
            || !headers_ok
        {
            return fail("blocked by the CORS preflight response");
        }
    }

    let mut url = job.url.clone();
    let mut method = method;
    let mut body = job.body.clone();
    for _ in 0..=MAX_REDIRECTS {
        if !review(&url) {
            return fail("the request was blocked");
        }
        let mut trusted = Vec::new();
        if cross || method != "GET" && method != "HEAD" {
            trusted.push(("Origin".to_string(), job.origin.clone()));
        }
        let hop = fetch_script_request_hop(
            &ScriptRequest {
                method: method.clone(),
                url: url.clone(),
                headers: job.headers.clone(),
                trusted_headers: trusted,
                body: body.clone(),
            },
            MAX_RESPONSE_BYTES,
        );
        match hop {
            Ok(ScriptHop::Redirect { location, status }) => {
                if origin_of(&location).is_none() {
                    return fail("redirected to a URL that cannot be fetched");
                }
                if matches!(status, 301..=303) && method != "GET" && method != "HEAD" {
                    method = "GET".into();
                    body.clear();
                }
                url = location;
            }
            Ok(ScriptHop::Response {
                status,
                status_text,
                headers,
                body,
            }) => {
                let final_cross = origin_of(&url).is_none_or(|o| o != job.origin);
                if final_cross && !allows_origin(&headers, &job.origin) {
                    return fail(
                        "blocked by CORS policy: no Access-Control-Allow-Origin allows this origin",
                    );
                }
                return NetOutcome::Done {
                    status,
                    status_text,
                    headers,
                    body,
                    url,
                };
            }
            Ok(ScriptHop::TooLarge) => return fail("the response is too large"),
            Err(error) => return fail(&error.to_string()),
        }
    }
    fail("too many redirects")
}

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b' ' | b'\n' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serves `replies` in order, one per connection.
    fn serve(replies: Vec<String>) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (origin, rx)
    }

    fn ok(extra: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn job(url: &str, origin: &str, method: &str, headers: Vec<(String, String)>) -> NetJob {
        NetJob {
            id: 1,
            method: method.into(),
            url: url.into(),
            headers,
            body: b"x".to_vec(),
            origin: origin.into(),
        }
    }

    #[test]
    fn a_same_origin_request_needs_no_cors() {
        let (origin, _rx) = serve(vec![ok("", "hello")]);
        let outcome = perform(
            &job(&format!("{origin}/a"), &origin, "GET", vec![]),
            &|_| true,
        );
        let NetOutcome::Done { status, body, .. } = outcome else {
            panic!()
        };
        assert_eq!((status, body.as_slice()), (200, &b"hello"[..]));
    }

    #[test]
    fn a_cross_origin_response_without_opt_in_is_blocked_and_with_it_allowed() {
        let (origin, _) = serve(vec![ok("", "secret")]);
        let outcome = perform(
            &job(
                &format!("{origin}/a"),
                "https://page.example",
                "GET",
                vec![],
            ),
            &|_| true,
        );
        assert!(matches!(outcome, NetOutcome::Failed(m) if m.contains("CORS")));
        let (origin, _) = serve(vec![ok(
            "Access-Control-Allow-Origin: https://page.example\r\n",
            "fine",
        )]);
        let outcome = perform(
            &job(
                &format!("{origin}/a"),
                "https://page.example",
                "GET",
                vec![],
            ),
            &|_| true,
        );
        assert!(matches!(outcome, NetOutcome::Done { .. }));
    }

    #[test]
    fn a_non_simple_cross_origin_request_is_preflighted() {
        let preflight = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: POST\r\nAccess-Control-Allow-Headers: content-type\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string();
        let (origin, rx) = serve(vec![
            preflight,
            ok("Access-Control-Allow-Origin: *\r\n", "{}"),
        ]);
        let outcome = perform(
            &job(
                &format!("{origin}/api"),
                "https://page.example",
                "POST",
                vec![("Content-Type".into(), "application/json".into())],
            ),
            &|_| true,
        );
        assert!(matches!(outcome, NetOutcome::Done { status: 200, .. }));
        let first = rx.recv().unwrap().to_ascii_lowercase();
        assert!(first.starts_with("options /api"), "{first}");
        assert!(first.contains("access-control-request-method: post"));
        assert!(first.contains("access-control-request-headers: content-type"));
        let second = rx.recv().unwrap().to_ascii_lowercase();
        assert!(second.starts_with("post /api") && second.contains("origin: https://page.example"));
    }

    #[test]
    fn a_refused_preflight_stops_before_the_request() {
        let (origin, _) = serve(vec![ok("", "no cors headers")]);
        let outcome = perform(
            &job(
                &format!("{origin}/api"),
                "https://page.example",
                "PUT",
                vec![],
            ),
            &|_| true,
        );
        assert!(matches!(outcome, NetOutcome::Failed(m) if m.contains("preflight")));
    }

    #[test]
    fn every_hop_is_reviewed_and_a_refusal_stops_the_request() {
        let seen = std::sync::Mutex::new(Vec::new());
        let (origin, _) = serve(vec![format!(
            "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "http://127.0.0.1:1/after"
        )]);
        let outcome = perform(
            &job(&format!("{origin}/start"), &origin, "GET", vec![]),
            &|url| {
                seen.lock().unwrap().push(url.to_string());
                !url.contains("/after")
            },
        );
        assert!(matches!(outcome, NetOutcome::Failed(m) if m.contains("blocked")));
        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[1].ends_with("/after"));
        // And a refused first hop never connects.
        let outcome = perform(
            &job("http://127.0.0.1:1/x", "http://127.0.0.1:1", "GET", vec![]),
            &|_| false,
        );
        assert!(matches!(outcome, NetOutcome::Failed(_)));
    }

    #[test]
    fn non_http_schemes_are_refused() {
        let outcome = perform(
            &job("file:///etc/passwd", "https://p.example", "GET", vec![]),
            &|_| true,
        );
        assert!(matches!(outcome, NetOutcome::Failed(_)));
    }

    #[test]
    fn base64_round_trips_any_bytes() {
        for bytes in [&b""[..], b"a", b"ab", b"abc", &[0, 255, 128, 7, 9][..]] {
            assert_eq!(base64_decode(&base64_encode(bytes)).unwrap(), bytes);
        }
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert!(base64_decode("@@").is_none());
    }
}
