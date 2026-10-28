// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fetches the `<link rel=stylesheet>` files of a page whose HTML has already
//! cleared both gatekeeper stages. Runs on the navigation's background thread,
//! like the fetch it follows, so a slow stylesheet delays only that tab's
//! navigation.
//!
//! A stylesheet is optional content: any failure (review refusal, network
//! error, wrong type, too large, over budget) leaves that sheet out and the
//! page is styled by what did load. Every connection, including each redirect
//! hop, is reviewed with `review` before it is opened.

use crate::stylesheet::{stylesheet_link_hrefs, LinkedStyles};
use url::Url;

/// What one page may load: a bounded number of sheets, each and all together
/// bounded in bytes, following a bounded number of redirects.
#[derive(Clone, Copy)]
pub(crate) struct SheetLimits {
    pub sheets: usize,
    pub bytes_each: usize,
    pub bytes_total: usize,
    pub redirects: usize,
}

pub(crate) const DEFAULT_SHEET_LIMITS: SheetLimits = SheetLimits {
    sheets: 16,
    bytes_each: 2 * 1024 * 1024,
    bytes_total: 6 * 1024 * 1024,
    redirects: 3,
};

/// The document's base URL: `document_url`, moved by the first `<base href>`.
fn base_url(doc: &blueice_dom::Document, document_url: &str) -> Option<Url> {
    fn first_base(doc: &blueice_dom::Document, node: blueice_dom::NodeId) -> Option<String> {
        if let blueice_dom::NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            if tag_name == "base" {
                if let Some((_, href)) = attributes.iter().find(|(name, _)| name == "href") {
                    return Some(href.clone());
                }
            }
        }
        doc.children(node)
            .into_iter()
            .find_map(|child| first_base(doc, child))
    }
    let document = Url::parse(document_url).ok()?;
    match first_base(doc, doc.root()) {
        Some(href) => document.join(href.trim()).ok().or(Some(document)),
        None => Some(document),
    }
}

/// Fetches the sheets `html` links, as `href`-as-written to CSS text.
/// `review` answers whether one absolute URL may be requested.
pub(crate) fn fetch_linked_styles(
    html: &str,
    document_url: &str,
    review: &(dyn Fn(&str) -> bool + Sync),
    limits: SheetLimits,
) -> LinkedStyles {
    let doc = blueice_html::parse(html);
    let Some(base) = base_url(&doc, document_url) else {
        return LinkedStyles::new();
    };
    let wanted: Vec<(String, String)> = stylesheet_link_hrefs(&doc)
        .into_iter()
        .filter_map(|href| {
            let absolute = base.join(&href).ok()?;
            matches!(absolute.scheme(), "http" | "https").then(|| (href, absolute.to_string()))
        })
        .take(limits.sheets)
        .collect();
    let fetched: Vec<Option<String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = wanted
            .iter()
            .map(|(_, url)| scope.spawn(move || fetch_one(url, review, limits)))
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().ok().flatten())
            .collect()
    });
    let mut total = 0;
    let mut sheets = LinkedStyles::new();
    for ((href, _), css) in wanted.into_iter().zip(fetched) {
        let Some(css) = css else { continue };
        total += css.len();
        if total > limits.bytes_total {
            break;
        }
        sheets.insert(href, css);
    }
    sheets
}

fn fetch_one(
    start: &str,
    review: &(dyn Fn(&str) -> bool + Sync),
    limits: SheetLimits,
) -> Option<String> {
    let mut url = start.to_string();
    for _ in 0..=limits.redirects {
        if !review(&url) {
            return None;
        }
        match blueice_net::fetch_subresource_hop(&url, limits.bytes_each).ok()? {
            blueice_net::SubresourceHop::Redirect { location, .. } => url = location,
            blueice_net::SubresourceHop::Response {
                status,
                content_type,
                body,
                ..
            } => {
                let css = content_type
                    .as_deref()
                    .is_some_and(|kind| kind.trim().to_ascii_lowercase().starts_with("text/css"));
                return ((200..300).contains(&status) && css)
                    .then(|| String::from_utf8_lossy(&body).into_owned());
            }
            blueice_net::SubresourceHop::TooLarge => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// path -> (status line, content type, extra header, body)
    type Route = (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        String,
    );

    /// Serves `routes` until the test ends and counts requests per server.
    fn serve(routes: Vec<Route>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counted = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buffer = [0u8; 2048];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                counted.fetch_add(1, Ordering::SeqCst);
                let reply = routes
                    .iter()
                    .find(|(p, ..)| *p == path)
                    .map(|(_, status, kind, header, body)| {
                        format!(
                            "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\n{header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    })
                    .unwrap_or_else(|| {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_string()
                    });
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        (origin, hits)
    }

    fn css(path: &'static str, body: &str) -> Route {
        (
            path,
            "200 OK",
            "text/css; charset=utf-8",
            "",
            body.to_string(),
        )
    }
    fn allow_all(_: &str) -> bool {
        true
    }
    fn fetch(html: &str, url: &str, review: &(dyn Fn(&str) -> bool + Sync)) -> LinkedStyles {
        fetch_linked_styles(html, url, review, DEFAULT_SHEET_LIMITS)
    }

    #[test]
    fn fetches_each_linked_sheet_keyed_by_the_href_as_written() {
        let (origin, _) = serve(vec![
            css("/a.css", "p{color:red}"),
            css("/dir/b.css", "b{}"),
        ]);
        let sheets = fetch(
            "<link rel=stylesheet href=/a.css><link rel=stylesheet href=b.css>",
            &format!("{origin}/dir/page.html"),
            &allow_all,
        );
        assert_eq!(
            sheets.get("/a.css").map(String::as_str),
            Some("p{color:red}")
        );
        assert_eq!(sheets.get("b.css").map(String::as_str), Some("b{}"));
        assert_eq!(sheets.len(), 2);
    }

    #[test]
    fn a_base_element_moves_where_relative_links_resolve() {
        let (origin, _) = serve(vec![css("/assets/site.css", "p{}")]);
        let sheets = fetch(
            "<base href=\"/assets/\"><link rel=stylesheet href=site.css>",
            &format!("{origin}/index.html"),
            &allow_all,
        );
        assert!(sheets.contains_key("site.css"));
    }

    #[test]
    fn a_sheet_that_is_not_css_or_not_ok_is_left_out() {
        let (origin, _) = serve(vec![
            ("/html.css", "200 OK", "text/html", "", "p{}".to_string()),
            (
                "/none.css",
                "200 OK",
                "application/octet-stream",
                "",
                "p{}".to_string(),
            ),
            (
                "/err.css",
                "500 Internal Server Error",
                "text/css",
                "",
                "p{}".to_string(),
            ),
            css("/ok.css", "p{}"),
        ]);
        let sheets = fetch(
            "<link rel=stylesheet href=/html.css><link rel=stylesheet href=/none.css>\
             <link rel=stylesheet href=/err.css><link rel=stylesheet href=/missing.css>\
             <link rel=stylesheet href=/ok.css>",
            &format!("{origin}/"),
            &allow_all,
        );
        assert_eq!(
            sheets.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["/ok.css"]
        );
    }

    #[test]
    fn a_refused_url_is_never_requested() {
        let (origin, hits) = serve(vec![css("/blocked.css", "p{}"), css("/fine.css", "p{}")]);
        let sheets = fetch(
            "<link rel=stylesheet href=/blocked.css><link rel=stylesheet href=/fine.css>",
            &format!("{origin}/"),
            &|url: &str| !url.contains("blocked"),
        );
        assert_eq!(
            sheets.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["/fine.css"]
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "only the allowed sheet was requested"
        );
    }

    #[test]
    fn every_redirect_hop_is_reviewed_before_it_is_requested() {
        let (origin, hits) = serve(vec![
            (
                "/go.css",
                "302 Found",
                "text/plain",
                "Location: /final.css\r\n",
                String::new(),
            ),
            css("/final.css", "p{color:red}"),
            (
                "/trap.css",
                "302 Found",
                "text/plain",
                "Location: /secret.css\r\n",
                String::new(),
            ),
            css("/secret.css", "p{}"),
        ]);
        let seen = std::sync::Mutex::new(Vec::new());
        let review = |url: &str| {
            seen.lock().unwrap().push(url.to_string());
            !url.contains("secret")
        };
        let sheets = fetch(
            "<link rel=stylesheet href=/go.css><link rel=stylesheet href=/trap.css>",
            &format!("{origin}/"),
            &review,
        );
        assert_eq!(
            sheets.get("/go.css").map(String::as_str),
            Some("p{color:red}")
        );
        assert!(!sheets.contains_key("/trap.css"));
        let seen = seen.lock().unwrap();
        assert!(seen.iter().any(|u| u.ends_with("/final.css")));
        assert!(
            seen.iter().any(|u| u.ends_with("/secret.css")),
            "the hop was reviewed"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            3,
            "go, final and trap; never secret"
        );
    }

    #[test]
    fn a_redirect_loop_gives_up() {
        let (origin, _) = serve(vec![(
            "/loop.css",
            "302 Found",
            "text/plain",
            "Location: /loop.css\r\n",
            String::new(),
        )]);
        assert!(fetch(
            "<link rel=stylesheet href=/loop.css>",
            &format!("{origin}/"),
            &allow_all
        )
        .is_empty());
    }

    #[test]
    fn size_and_count_limits_leave_sheets_out() {
        let (origin, _) = serve(vec![
            css("/big.css", &"a{}".repeat(100)),
            css("/s1.css", "p{}"),
            css("/s2.css", "p{}"),
            css("/s3.css", "p{}"),
        ]);
        let url = format!("{origin}/");
        let limits = SheetLimits {
            sheets: 3,
            bytes_each: 50,
            bytes_total: 5,
            redirects: 1,
        };
        let sheets = fetch_linked_styles(
            "<link rel=stylesheet href=/big.css><link rel=stylesheet href=/s1.css>\
             <link rel=stylesheet href=/s2.css><link rel=stylesheet href=/s3.css>",
            &url,
            &allow_all,
            limits,
        );
        // big is over the per-sheet limit; of the first three links only s1
        // and s2 remain, and 3 + 3 bytes exceeds the 5-byte total, so s2 goes.
        assert_eq!(
            sheets.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["/s1.css"]
        );
    }

    #[test]
    fn non_http_and_unparsable_links_are_ignored() {
        let sheets = fetch(
            "<link rel=stylesheet href=\"data:text/css,p{}\"><link rel=stylesheet href=\"file:///etc/x.css\">",
            "http://127.0.0.1:9/",
            &allow_all,
        );
        assert!(sheets.is_empty());
        assert!(fetch("<link rel=stylesheet href=a.css>", "not a url", &allow_all).is_empty());
    }
}
