// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_net::{
    fetch_navigation_request_hop, FetchHop, FormEncoding, NavigationRequest, MAX_FORM_BODY_BYTES,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn post_hop_sends_bounded_private_bytes_and_exposes_error_html_for_review() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/submit?kept=1#section",
        listener.local_addr().unwrap()
    );
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 1024];
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
            assert!(request.len() <= 16384);
            if request.ends_with(b"q=secret") {
                break;
            }
        }
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with("POST /submit?kept=1 HTTP/1.1\r\n"));
        write!(stream, "HTTP/1.1 422 Unprocessable Content\r\nContent-Type: text/html\r\nContent-Length: 16\r\nConnection: close\r\n\r\n<p>Try again</p>").unwrap();
    });
    let request =
        NavigationRequest::post(url, FormEncoding::UrlEncoded, b"q=secret".to_vec()).unwrap();
    assert!(!format!("{request:?}").contains("secret"));
    match fetch_navigation_request_hop(&request).unwrap() {
        FetchHop::Page(page) => {
            assert_eq!(page.status, 422);
            assert_eq!(page.body, "<p>Try again</p>");
        }
        FetchHop::Redirect { .. } => panic!("expected error page, not redirect"),
    }
    server.join().unwrap();
}

#[test]
fn redirect_hop_does_not_open_the_location_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let destination = TcpListener::bind("127.0.0.1:0").unwrap();
    destination.set_nonblocking(true).unwrap();
    let url = format!("http://{}/submit", listener.local_addr().unwrap());
    let target = format!("http://{}/target", destination.local_addr().unwrap());
    let location = target.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut received = Vec::new();
        while !received.windows(4).any(|part| part == b"\r\n\r\n") {
            let mut data = [0; 4096];
            let count = stream.read(&mut data).unwrap();
            assert!(count > 0);
            received.extend_from_slice(&data[..count]);
            assert!(received.len() <= 16384);
        }
        write!(stream, "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let request = NavigationRequest::post(url, FormEncoding::UrlEncoded, Vec::new()).unwrap();
    assert!(
        matches!(fetch_navigation_request_hop(&request).unwrap(), FetchHop::Redirect { status: 307, location } if location == target)
    );
    assert_eq!(
        destination.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    server.join().unwrap();
}

#[test]
fn oversized_bodies_and_header_injecting_boundaries_are_rejected_without_data_in_errors() {
    let error = NavigationRequest::post(
        "https://example.test/",
        FormEncoding::UrlEncoded,
        vec![b'x'; MAX_FORM_BODY_BYTES + 1],
    )
    .unwrap_err();
    assert!(error.to_string().contains("limit"));
    let error = NavigationRequest::post(
        "https://example.test/",
        FormEncoding::Multipart {
            boundary: "secret\r\nX-Injected: yes".into(),
        },
        b"password=private".to_vec(),
    )
    .unwrap_err();
    assert!(!format!("{error:?}").contains("private"));
    assert!(!error.to_string().contains("secret"));
}
