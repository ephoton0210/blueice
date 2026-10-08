// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public navigation regressions for header-first original response downloads.
//! Assert classification, exact binary bytes and one-shot requests. Manager,
//! core ownership and native UI are covered at their respective boundaries.
//!
//! References: RFC 6266 sections 4.2/4.3; HTML navigation response handling.

#[path = "fixtures/navigation_response.rs"]
mod fixture;

use blueice_net::{fetch_navigation_request_hop, FetchHop, FormEncoding, NavigationRequest};
use fixture::ResponseServer;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn response_fixture_waits_for_a_request_sent_after_accept() {
    use std::io::{Read, Write};
    let server = ResponseServer::start("200 OK", &[("Content-Type", "text/html")], b"ready", false);
    let address = server
        .url
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut client = std::net::TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    thread::sleep(Duration::from_millis(50));
    let write = client.write_all(b"GET /response HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let mut response = Vec::new();
    let read = client.read_to_end(&mut response);
    let request = server.finish();
    write.unwrap();
    read.unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(request.target, "/response");
    assert!(request.body.is_empty());
    assert!(response.ends_with(b"\r\n\r\nready"));
}

fn assert_download_bytes(hop: FetchHop, expected: &[u8]) {
    use std::io::Read;
    let FetchHop::Download(download) = hop else {
        panic!("expected an owned download response")
    };
    assert_eq!(download.content_length, Some(expected.len() as u64));
    let mut bytes = Vec::new();
    download.into_reader().read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, expected);
}

#[test]
fn attachment_html_is_not_returned_as_a_page() {
    let server = ResponseServer::start(
        "200 OK",
        &[
            ("Content-Type", "text/html"),
            ("Content-Disposition", "attachment; filename=report.html"),
        ],
        b"<p>downloaded, not displayed</p>",
        false,
    );
    let result = fetch_navigation_request_hop(&NavigationRequest::get(&server.url));
    let request = server.finish();
    assert_eq!(request.method, "GET");
    assert_eq!(request.target, "/response");
    assert_download_bytes(result.unwrap(), b"<p>downloaded, not displayed</p>");
}

#[test]
fn binary_attachment_is_classified_without_utf8_decoding() {
    let server = ResponseServer::start(
        "200 OK",
        &[
            ("Content-Type", "application/octet-stream"),
            ("Content-Disposition", "attachment; filename=bytes.bin"),
        ],
        &[0, 0xff, 0x80, 0xfe, 13, 10, 1],
        false,
    );
    let result = fetch_navigation_request_hop(&NavigationRequest::get(&server.url));
    let _ = server.finish();
    assert_download_bytes(
        result.expect("download bytes must not pass through a text decoder"),
        &[0, 0xff, 0x80, 0xfe, 13, 10, 1],
    );
}

#[test]
fn attachment_headers_return_before_the_response_body_is_released() {
    let mut server = ResponseServer::start(
        "200 OK",
        &[
            ("Content-Type", "application/octet-stream"),
            ("Content-Disposition", "attachment"),
        ],
        &[0xff, 0, 0x80],
        true,
    );
    let request = NavigationRequest::get(&server.url);
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let _ = tx.send(fetch_navigation_request_hop(&request));
    });
    let headers = server.wait_for_headers();
    let early = if headers.is_ok() {
        Some(rx.recv_timeout(Duration::from_secs(2)))
    } else {
        None
    };
    server.release();
    worker.join().unwrap();
    let _ = server.finish();
    headers.unwrap();
    let hop = early
        .unwrap()
        .expect("a download response must return with its unread owned body")
        .unwrap();
    assert_download_bytes(hop, &[0xff, 0, 0x80]);
}

#[test]
fn mixed_case_attachment_without_filename_still_downloads() {
    let server = ResponseServer::start(
        "200 OK",
        &[
            ("Content-Type", "text/html; charset=utf-8"),
            ("Content-Disposition", "AtTaChMeNt"),
        ],
        b"<p>file</p>",
        false,
    );
    let result = fetch_navigation_request_hop(&NavigationRequest::get(&server.url));
    let _ = server.finish();
    assert_download_bytes(result.unwrap(), b"<p>file</p>");
}

#[test]
fn unknown_valid_disposition_is_handled_as_attachment() {
    let server = ResponseServer::start(
        "200 OK",
        &[
            ("Content-Type", "text/html"),
            (
                "Content-Disposition",
                "example-disposition; filename=report.html",
            ),
        ],
        b"<p>file</p>",
        false,
    );
    let result = fetch_navigation_request_hop(&NavigationRequest::get(&server.url));
    let _ = server.finish();
    assert_download_bytes(result.unwrap(), b"<p>file</p>");
}

#[test]
fn inline_html_with_filename_retains_page_handling() {
    let body = "<p>preserve the document</p>";
    let server = ResponseServer::start(
        "200 OK",
        &[
            ("Content-Type", "text/html"),
            ("Content-Disposition", "inline; filename=report.html"),
        ],
        body.as_bytes(),
        false,
    );
    let result = fetch_navigation_request_hop(&NavigationRequest::get(&server.url));
    let _ = server.finish();
    let FetchHop::Page(page) = result.unwrap() else {
        panic!("inline HTML must remain a page")
    };
    assert_eq!(page.body, body);
    assert_eq!(page.status, 200);
}

#[test]
fn attachment_header_does_not_skip_redirect_policy_review() {
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let location = format!("http://{}/next", destination.local_addr().unwrap());
    let server = ResponseServer::start(
        "302 Found",
        &[
            ("Location", &location),
            ("Content-Type", "text/html"),
            ("Content-Disposition", "attachment; filename=redirect.html"),
        ],
        b"",
        false,
    );
    let result = fetch_navigation_request_hop(&NavigationRequest::get(&server.url));
    let _ = server.finish();
    assert!(
        matches!(result.unwrap(), FetchHop::Redirect { status: 302, location: target } if target == location)
    );
    assert_eq!(
        destination.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn post_attachment_keeps_the_original_post_request() {
    let server = ResponseServer::start(
        "201 Created",
        &[
            ("Content-Type", "text/html"),
            ("Content-Disposition", "attachment; filename=receipt.html"),
        ],
        b"<p>one-shot receipt</p>",
        false,
    );
    let body = b"token=test-fixture&name=download".to_vec();
    let request =
        NavigationRequest::post(&server.url, FormEncoding::UrlEncoded, body.clone()).unwrap();
    let result = fetch_navigation_request_hop(&request);
    let captured = server.finish();
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.target, "/response");
    assert_eq!(captured.body, body);
    assert_download_bytes(result.unwrap(), b"<p>one-shot receipt</p>");
}

#[test]
fn unsupported_media_type_downloads_without_a_disposition() {
    for mime in ["application/octet-stream", "application/pdf", "image/png"] {
        let bytes = [0, 0xff, 0x80, 42];
        let server = ResponseServer::start("200 OK", &[("Content-Type", mime)], &bytes, false);
        let hop = fetch_navigation_request_hop(&NavigationRequest::get(&server.url)).unwrap();
        let _ = server.finish();
        assert_download_bytes(hop, &bytes);
    }
}

#[test]
fn download_metadata_retains_extended_unicode_filename_and_original_status() {
    let disposition = "attachment; filename=plain.txt; filename*=UTF-8''%E4%B8%8B%E8%BC%89.txt";
    let server = ResponseServer::start(
        "201 Created",
        &[
            ("Content-Type", "text/plain"),
            ("Content-Disposition", disposition),
        ],
        b"original",
        false,
    );
    let hop = fetch_navigation_request_hop(&NavigationRequest::get(&server.url)).unwrap();
    let FetchHop::Download(ref download) = hop else {
        panic!("expected download")
    };
    assert_eq!(download.status, 201);
    assert_eq!(download.final_url, server.url);
    assert_eq!(download.content_type.as_deref(), Some("text/plain"));
    assert_eq!(download.content_disposition.as_deref(), Some(disposition));
    assert_eq!(
        blueice_net::download::file_name::choose_file_name(
            download.content_disposition.as_deref(),
            &download.final_url
        ),
        "下載.txt"
    );
    let _ = server.finish();
    assert_download_bytes(hop, b"original");
}
