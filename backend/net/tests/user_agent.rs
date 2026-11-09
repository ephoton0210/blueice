// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every kind of request BlueIce makes for a page says the same thing about who is asking
//! (BlueIce, not the HTTP library's own default `ureq/3`), and it is what the page's scripts
//! read in `navigator.userAgent`.

use blueice_net::{fetch, fetch_script_request_hop, fetch_subresource_hop, ScriptRequest, USER_AGENT};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

/// A server that answers one request and reports the headers it got.
fn one_shot() -> (String, mpsc::Receiver<Vec<(String, String)>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).unwrap_or(0) == 0 {
                break;
            }
            head.push(byte[0]);
        }
        let headers = String::from_utf8_lossy(&head)
            .lines()
            .skip(1)
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        );
        let _ = tx.send(headers);
    });
    (url, rx)
}

fn agent_of(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .find(|(k, _)| k == "user-agent")
        .map(|(_, v)| v.clone())
        .expect("a User-Agent header")
}

#[test]
fn a_navigation_a_subresource_and_a_scripts_request_all_send_the_browsers_user_agent() {
    let (url, headers) = one_shot();
    fetch(&url).expect("navigation");
    assert_eq!(agent_of(&headers.recv().unwrap()), USER_AGENT);

    let (url, headers) = one_shot();
    fetch_subresource_hop(&url, 1024).expect("subresource");
    assert_eq!(agent_of(&headers.recv().unwrap()), USER_AGENT);

    let (url, headers) = one_shot();
    let request = ScriptRequest {
        method: "GET".into(),
        url,
        headers: vec![],
        trusted_headers: vec![],
        body: vec![],
    };
    fetch_script_request_hop(&request, 1024).expect("script request");
    assert_eq!(agent_of(&headers.recv().unwrap()), USER_AGENT);
}

#[test]
fn the_user_agent_is_a_browser_style_string_that_names_blueice() {
    assert!(USER_AGENT.starts_with("Mozilla/5.0 ("), "{USER_AGENT}");
    assert!(USER_AGENT.contains("BlueIce/"), "{USER_AGENT}");
    assert!(!USER_AGENT.to_ascii_lowercase().contains("ureq"));
}
