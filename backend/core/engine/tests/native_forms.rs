// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![cfg(unix)]

use blueice_engine::{session::run_session, TabManager};
use blueice_ipc::gatekeeper::{GatekeeperReply, GatekeeperRequest};
use blueice_ipc::input::{PageKey, TextInputAction, TextInputContext, TextInputState};
use blueice_ipc::{AiSnapshot, ClientMessage, NodeAction, ServerMessage};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Clone, Debug)]
struct HttpRequest {
    method: String,
    target: String,
    headers: String,
    body: String,
}

fn read_http(mut stream: TcpStream) -> HttpRequest {
    // macOS accepted sockets inherit the listener's nonblocking flag.
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() <= 16_384, "fixture header is bounded");
        if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break offset + 4;
        }
    };
    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    assert!(length <= 1_048_576, "fixture request body is bounded");
    while bytes.len() < header_end + length {
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
    }
    let mut first = headers.lines().next().unwrap().split_whitespace();
    let request = HttpRequest {
        method: first.next().unwrap().into(),
        target: first.next().unwrap().into(),
        body: String::from_utf8(bytes[header_end..header_end + length].to_vec()).unwrap(),
        headers,
    };
    if request.target == "/slow" {
        thread::sleep(Duration::from_millis(200));
    }
    let body = if request.target == "/new-form" {
        "<form method=post action='/posted'><input name=q value=secret><button aria-label=Submit>Send</button></form>"
    } else {
        "<h1>Form received</h1>"
    };
    if let Some(status) = request
        .target
        .strip_prefix("/redirect-")
        .and_then(|suffix| suffix.parse::<u16>().ok())
    {
        write!(stream, "HTTP/1.1 {status} Redirect\r\nLocation: /received\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        return request;
    }
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    request
}

struct Browser {
    client: UnixStream,
    state: TextInputState,
    root: PathBuf,
    running: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
    reviews: Arc<Mutex<Vec<GatekeeperRequest>>>,
}

impl Browser {
    fn new(html: &str) -> Self {
        Self::with_rules(html, false)
    }
    fn with_rules(html: &str, compiled: bool) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "bi-form-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let http = TcpListener::bind("127.0.0.1:0").unwrap();
        http.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", http.local_addr().unwrap());
        let gatekeeper = UnixListener::bind(root.join("gate.sock")).unwrap();
        gatekeeper.set_nonblocking(true).unwrap();
        let running = Arc::new(AtomicBool::new(true));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let reviews = Arc::new(Mutex::new(Vec::new()));
        let mut workers = Vec::new();
        let active = running.clone();
        let recorded = requests.clone();
        workers.push(thread::spawn(move || {
            while active.load(Ordering::Acquire) {
                match http.accept() {
                    Ok((stream, _)) => recorded.lock().unwrap().push(read_http(stream)),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        }));
        let active = running.clone();
        let recorded = reviews.clone();
        workers.push(thread::spawn(move || {
            while active.load(Ordering::Acquire) {
                match gatekeeper.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        let request =
                            blueice_ipc::gatekeeper::read_gatekeeper_request(&mut stream).unwrap();
                        let reply = if compiled { blueice_ai_gatekeeper::review(&request) }
                            else if matches!(&request, GatekeeperRequest::CheckUrl { url } if url.ends_with("/denied")) { GatekeeperReply::Rejected { reason: "test denied redirect".into(), category: "test".into() } }
                            else { GatekeeperReply::Cleared };
                        recorded.lock().unwrap().push(request);
                        blueice_ipc::gatekeeper::write_gatekeeper_reply(
                            &mut stream,
                            &reply,
                        )
                        .unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        }));
        let mut tabs = TabManager::new(600.0, 400.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab)
            .unwrap()
            .load_html_str(html, Some(format!("{origin}/form")));
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let directory = root.clone();
        workers.push(thread::spawn(move || {
            run_session(
                &mut tabs,
                &mut server,
                &directory,
                &mut 0,
                &directory.join("gate.sock"),
            )
            .unwrap();
        }));
        blueice_ipc::client_handshake(&mut client).unwrap();
        blueice_ipc::write_client_message(&mut client, &ClientMessage::GetTextInputState).unwrap();
        let ServerMessage::TextInputState(state) =
            blueice_ipc::read_server_message(&mut client).unwrap()
        else {
            panic!("input state required")
        };
        Self {
            client,
            state,
            root,
            running,
            workers,
            requests,
            reviews,
        }
    }

    fn send(&mut self, command: ClientMessage) {
        blueice_ipc::write_client_message(&mut self.client, &command).unwrap();
    }

    fn snapshot(&mut self) -> AiSnapshot {
        self.send(ClientMessage::GetRepresentation);
        loop {
            match blueice_ipc::read_server_message(&mut self.client).unwrap() {
                ServerMessage::Representation(snapshot) => return snapshot,
                ServerMessage::FrameReady { .. } | ServerMessage::NavigationStarted { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    fn focus(&mut self, name: &str) {
        let id = self
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(name))
            .unwrap()
            .id;
        self.send(ClientMessage::ActOn {
            id,
            action: NodeAction::Focus,
        });
        self.send(ClientMessage::GetTextInputState);
        loop {
            match blueice_ipc::read_server_message(&mut self.client).unwrap() {
                ServerMessage::TextInputState(state) => {
                    self.state = state;
                    return;
                }
                ServerMessage::FrameReady { .. } | ServerMessage::NavigationStarted { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    fn input(&mut self, action: TextInputAction) {
        self.send(ClientMessage::TextInput {
            context: TextInputContext {
                version: self.state.version,
                frame_source: self.state.frame_source,
                document_generation: self.state.document_generation,
                focus_generation: self.state.focus_generation,
            },
            action,
        });
        loop {
            match blueice_ipc::read_server_message(&mut self.client).unwrap() {
                ServerMessage::TextInputState(state) => {
                    self.state = state;
                    return;
                }
                ServerMessage::FrameReady { .. } | ServerMessage::NavigationStarted { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    fn key(&mut self, key: PageKey) {
        self.input(TextInputAction::Key { key, shift: false });
    }

    fn submit(&mut self, name: &str) {
        self.focus(name);
        self.key(PageKey::Enter);
        loop {
            match blueice_ipc::read_server_message(&mut self.client) {
                Ok(ServerMessage::Navigated { .. }) => return,
                Ok(ServerMessage::FrameReady { .. } | ServerMessage::NavigationStarted { .. }) => {}
                other => panic!("Expected actual reviewed form navigation, received {other:?}"),
            }
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.client.shutdown(std::net::Shutdown::Both);
        let mut failed = self.workers.pop().expect("session worker").join().is_err();
        self.running.store(false, Ordering::Release);
        for worker in self.workers.drain(..) {
            failed |= worker.join().is_err();
        }
        let cleanup = std::fs::remove_dir_all(&self.root);
        if !thread::panicking() {
            assert!(!failed, "form fixture worker failed");
            cleanup.unwrap();
        }
    }
}

#[test]
fn get_form_encodes_successful_controls_in_document_order_and_replaces_action_query() {
    let mut browser = Browser::new(
        r#"
      <form id='f' action='/received?old=discarded#section'>
        <input name='q' value='A & 冰' aria-label='Query'>
        <input name='same' value='first' readonly><input name='same' value='second'>
        <input name='disabled' value='excluded' disabled>
        <input type='checkbox' name='checked' checked><input type='checkbox' name='unchecked'>
        <select name='region'><option value='a'>Alpha</option><option value='b' selected>Beta</option></select>
        <textarea name='notes'>one
two</textarea><input type='hidden' name='_charset_' value='ignored'>
        <button aria-label='Submit' name='mode' value='save'>Send</button>
        <button name='other' value='excluded'>Other</button>
      </form><input form='f' name='outside' value='yes'>"#,
    );
    browser.submit("Submit");
    let requests = browser.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].target, "/received?q=A+%26+%E5%86%B0&same=first&same=second&checked=on&region=b&notes=one%0D%0Atwo&_charset_=UTF-8&mode=save&outside=yes");
    assert!(requests[0].body.is_empty());
}

#[test]
fn submitter_post_overrides_keep_action_query_and_send_values_in_the_http_body() {
    let mut browser = Browser::new(
        r#"
      <form action='/wrong' method='get'><input name='q' value='A&amp;B'>
      <button aria-label='Submit' formaction='/posted?kept=1' formmethod='post' name='mode' value='save'>Send</button></form>"#,
    );
    browser.submit("Submit");
    let requests = browser.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].target, "/posted?kept=1");
    assert_eq!(requests[0].body, "q=A%26B&mode=save");
    assert!(requests[0]
        .headers
        .to_ascii_lowercase()
        .contains("content-type: application/x-www-form-urlencoded"));
    assert!(browser
        .reviews
        .lock()
        .unwrap()
        .iter()
        .all(|review| !format!("{review:?}").contains("q=A%26B")));
}

impl Browser {
    fn click_reply(&mut self, label: &str) -> ServerMessage {
        let id = self
            .snapshot()
            .nodes
            .iter()
            .find(|node| node.name.as_deref() == Some(label))
            .unwrap()
            .id;
        self.send(ClientMessage::ActOn {
            id,
            action: NodeAction::Click,
        });
        loop {
            match blueice_ipc::read_server_message(&mut self.client).unwrap() {
                ServerMessage::FrameReady { .. } | ServerMessage::NavigationStarted { .. } => {}
                other => return other,
            }
        }
    }
    fn navigated(&mut self) {
        loop {
            match blueice_ipc::read_server_message(&mut self.client).unwrap() {
                ServerMessage::Navigated { .. } => return,
                ServerMessage::FrameReady { .. }
                | ServerMessage::NavigationStarted { .. }
                | ServerMessage::FormResubmissionResolved { .. } => {}
                other => panic!("unexpected navigation reply {other:?}"),
            }
        }
    }
    fn prompt(&mut self) -> u64 {
        loop {
            match blueice_ipc::read_server_message(&mut self.client).unwrap() {
                ServerMessage::FormResubmission {
                    confirmation_id,
                    url,
                } => {
                    assert!(!url.contains("secret"));
                    return confirmation_id;
                }
                ServerMessage::FrameReady { .. } | ServerMessage::NavigationStarted { .. } => {}
                other => panic!("expected POST confirmation, received {other:?}"),
            }
        }
    }
}

#[test]
fn post_reload_cancel_has_no_request_and_accept_reuses_body_without_new_history() {
    let mut browser = Browser::new("<form method=post action='/posted'><input name=q value=secret><button aria-label=Submit>Send</button></form>");
    browser.submit("Submit");
    assert!(!format!("{:?}", browser.snapshot()).contains("secret"));
    browser.send(ClientMessage::Reload);
    let confirmation_id = browser.prompt();
    assert_eq!(browser.requests.lock().unwrap().len(), 1);
    browser.send(ClientMessage::ConfirmFormResubmission {
        confirmation_id,
        accept: false,
    });
    assert!(matches!(
        blueice_ipc::read_server_message(&mut browser.client).unwrap(),
        ServerMessage::FormResubmissionResolved {
            accepted: false,
            ..
        }
    ));
    assert_eq!(browser.requests.lock().unwrap().len(), 1);
    browser.send(ClientMessage::Reload);
    let confirmation_id = browser.prompt();
    browser.send(ClientMessage::ConfirmFormResubmission {
        confirmation_id,
        accept: true,
    });
    browser.navigated();
    assert_eq!(browser.requests.lock().unwrap().len(), 2);
    assert!(browser
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.method == "POST" && request.body == "q=secret"));
    browser.send(ClientMessage::GoBack);
    browser.navigated();
    assert_eq!(
        browser.requests.lock().unwrap().last().unwrap().target,
        "/form"
    );
    browser.send(ClientMessage::GoForward);
    let confirmation_id = browser.prompt();
    browser.send(ClientMessage::ConfirmFormResubmission {
        confirmation_id,
        accept: true,
    });
    browser.navigated();
    assert_eq!(
        browser.requests.lock().unwrap().last().unwrap().body,
        "q=secret"
    );
}

#[test]
fn resubmission_confirmation_is_single_use_and_superseded_by_navigation() {
    let mut browser = Browser::new(
        "<form method=post action='/posted'><button aria-label=Submit>Send</button></form>",
    );
    browser.submit("Submit");
    browser.send(ClientMessage::Reload);
    let confirmation_id = browser.prompt();
    browser.send(ClientMessage::Navigate {
        url: "about:blank".into(),
    });
    browser.navigated();
    browser.send(ClientMessage::ConfirmFormResubmission {
        confirmation_id,
        accept: true,
    });
    loop {
        match blueice_ipc::read_server_message(&mut browser.client).unwrap() {
            ServerMessage::Error { message } => {
                assert!(message.contains("stale"));
                break;
            }
            ServerMessage::FrameReady { .. } | ServerMessage::NavigationStarted { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(browser.requests.lock().unwrap().len(), 1);
}

#[test]
fn post_redirect_methods_and_review_order_are_applied_before_each_connection() {
    for status in [301, 302, 303, 307, 308] {
        let mut browser = Browser::with_rules(&format!("<form method=post action='/redirect-{status}'><input name=q value=secret><button aria-label=Submit>Send</button></form>"), true);
        browser.submit("Submit");
        let requests = browser.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(
            requests[1].method,
            if status < 307 { "GET" } else { "POST" }
        );
        assert_eq!(requests[1].body, if status < 307 { "" } else { "q=secret" });
        let reviews = browser.reviews.lock().unwrap();
        assert!(
            matches!(&reviews[0], GatekeeperRequest::CheckFormSubmission { method, .. } if method == "POST")
        );
        assert!(matches!(&reviews[1], GatekeeperRequest::CheckUrl { .. }));
        assert!(
            matches!(&reviews[2], GatekeeperRequest::CheckFormSubmission { method, .. } if method == requests[1].method.as_str())
        );
        assert!(matches!(&reviews[3], GatekeeperRequest::CheckUrl { .. }));
        assert!(matches!(
            &reviews[4],
            GatekeeperRequest::CheckContent { .. }
        ));
        assert!(reviews
            .iter()
            .all(|review| !format!("{review:?}").contains("secret")));
        drop(reviews);
        drop(requests);
        // A POST -> GET redirect is a GET history entry and reloads directly.
        browser.send(ClientMessage::Reload);
        if status < 307 {
            browser.navigated();
            assert_eq!(
                browser.requests.lock().unwrap().last().unwrap().method,
                "GET"
            );
        } else {
            browser.prompt();
            assert_eq!(browser.requests.lock().unwrap().len(), 2);
        }
    }
}

#[test]
fn required_validation_prevents_http_and_submitter_can_opt_out() {
    let mut browser = Browser::new("<form action='/received'><input name=q required><button aria-label=Submit>Send</button><button aria-label=Bypass formnovalidate>Bypass</button></form>");
    assert!(
        matches!(browser.click_reply("Submit"), ServerMessage::Error { message } if message.contains("required"))
    );
    assert!(browser.requests.lock().unwrap().is_empty());
    assert!(browser.reviews.lock().unwrap().is_empty());
    browser.submit("Bypass");
    assert_eq!(browser.requests.lock().unwrap()[0].target, "/received?q=");
}

#[test]
fn protected_get_never_encodes_credentials_and_compiled_rules_reject_insecure_post() {
    for method in ["get", "post"] {
        let mut browser = Browser::with_rules(&format!("<form action='/received' method='{method}'><input type=password name=p value=secret><button aria-label=Submit>Send</button></form>"), true);
        let reply = browser.click_reply("Submit");
        assert!(matches!(
            reply,
            ServerMessage::Error { .. } | ServerMessage::GatekeeperBlocked { .. }
        ));
        assert!(!format!("{reply:?}").contains("secret"));
        assert!(browser.requests.lock().unwrap().is_empty());
        assert!(browser
            .reviews
            .lock()
            .unwrap()
            .iter()
            .all(|review| !format!("{review:?}").contains("secret")));
    }
}

#[test]
fn plain_and_multipart_encodings_preserve_order_and_empty_file_never_reads_a_path() {
    for encoding in ["text/plain", "multipart/form-data"] {
        let mut browser = Browser::new(&format!("<form action='/received' method=post enctype='{encoding}'><input name=q value='A&amp;B'><textarea name=notes>one\ntwo</textarea><input type=file name=upload value='/etc/passwd'><button aria-label=Submit>Send</button></form>"));
        browser.submit("Submit");
        let requests = browser.requests.lock().unwrap();
        let request = &requests[0];
        if encoding == "text/plain" {
            assert_eq!(request.body, "q=A&B\r\nnotes=one\r\ntwo\r\nupload=\r\n");
        } else {
            let boundary = request
                .headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-type: multipart/form-data; boundary=")
                        .or_else(|| {
                            line.strip_prefix("Content-Type: multipart/form-data; boundary=")
                        })
                })
                .unwrap();
            assert!(request.body.starts_with(&format!("--{boundary}\r\n")));
            assert!(request.body.ends_with(&format!("--{boundary}--\r\n")));
            assert!(request.body.contains("name=\"upload\"; filename=\"\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n"));
            assert!(request.body.contains("one\r\ntwo"));
        }
        assert!(!request.body.contains("/etc/passwd"));
    }
}

#[test]
fn implicit_enter_submits_a_single_field_without_a_button_and_uses_document_base() {
    let mut browser = Browser::new("<base href='/directory/'><form action='received'><input aria-label=Query name=q value=ice></form>");
    browser.focus("Query");
    browser.key(PageKey::Enter);
    browser.navigated();
    assert_eq!(
        browser.requests.lock().unwrap()[0].target,
        "/directory/received?q=ice"
    );
}

#[test]
fn multi_select_disabled_fieldsets_readonly_and_required_nameless_radios_are_correct() {
    let mut browser = Browser::new("<form action='/received'><fieldset disabled><legend><input name=legend value=included></legend><input name=disabled value=no></fieldset><input name=readonly value=yes readonly required><select name=region multiple required><option selected value=a>Alpha</option><option selected value=b>Beta</option><optgroup disabled><option selected value=c>Excluded</option></optgroup></select><button aria-label=Submit required>Send</button></form>");
    browser.submit("Submit");
    assert_eq!(
        browser.requests.lock().unwrap()[0].target,
        "/received?legend=included&readonly=yes&region=a&region=b"
    );
    let mut browser = Browser::new("<form><input type=radio required><input type=radio checked><button aria-label=Submit>Send</button></form>");
    assert!(matches!(
        browser.click_reply("Submit"),
        ServerMessage::Error { .. }
    ));
    assert!(browser.requests.lock().unwrap().is_empty());
}

#[test]
fn post_history_budget_expires_old_bodies_without_silent_get_fallback() {
    let mut browser = Browser::new("<form method=post action='/posted'><input name=q value=secret><button aria-label=Submit>Send</button></form>");
    browser.submit("Submit");
    for _ in 0..9 {
        let base = browser.snapshot().url.unwrap();
        browser.send(ClientMessage::Navigate {
            url: url::Url::parse(&base)
                .unwrap()
                .join("/new-form")
                .unwrap()
                .to_string(),
        });
        browser.navigated();
        browser.submit("Submit");
    }
    for _ in 0..7 {
        browser.send(ClientMessage::GoBack);
        browser.navigated();
        browser.send(ClientMessage::GoBack);
        let confirmation_id = browser.prompt();
        browser.send(ClientMessage::ConfirmFormResubmission {
            confirmation_id,
            accept: true,
        });
        browser.navigated();
    }
    browser.send(ClientMessage::GoBack);
    browser.navigated();
    let before = browser.requests.lock().unwrap().len();
    browser.send(ClientMessage::GoBack);
    loop {
        match blueice_ipc::read_server_message(&mut browser.client).unwrap() {
            ServerMessage::Error { message } => {
                assert!(message.contains("expired"));
                break;
            }
            ServerMessage::FrameReady { .. } | ServerMessage::NavigationStarted { .. } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(browser.requests.lock().unwrap().len(), before);
}

#[test]
fn a_second_activation_while_the_same_form_is_pending_does_not_duplicate_the_request() {
    let mut browser = Browser::new("<form method=post action='/slow'><input name=q value=secret><button aria-label=Submit>Send</button></form>");
    let id = browser
        .snapshot()
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("Submit"))
        .unwrap()
        .id;
    for _ in 0..2 {
        browser.send(ClientMessage::ActOn {
            id,
            action: NodeAction::Click,
        });
    }
    browser.navigated();
    assert_eq!(browser.requests.lock().unwrap().len(), 1);
}

#[test]
fn required_select_distinguishes_placeholder_multiple_and_listbox_selection() {
    for (select, valid, query) in [
        ("<select name=s required><option value=''>Choose</option><option value=b>Beta</option></select>", false, ""),
        ("<select name=s required multiple><option selected value=''>Empty value</option></select>", true, "q=ice&s="),
        ("<select name=s required size=2><option value=a>Alpha</option></select>", false, ""),
        ("<select name=s required><option selected disabled value=a>Disabled</option></select>", true, "q=ice"),
        ("<select name=s required><optgroup label=Group><option selected value=''>Empty grouped value</option></optgroup></select>", true, "q=ice&s="),
    ] {
        let mut browser = Browser::new(&format!("<form action='/received'><input name=q value=ice>{select}<button aria-label=Submit>Send</button></form>"));
        if valid { browser.submit("Submit"); assert_eq!(browser.requests.lock().unwrap()[0].target, format!("/received?{query}")); }
        else { assert!(matches!(browser.click_reply("Submit"), ServerMessage::Error { .. })); assert!(browser.requests.lock().unwrap().is_empty()); }
    }
}

#[test]
fn excessive_successful_controls_and_utf8_expansion_are_rejected_before_review_or_network() {
    for fields in [
        "<input type=hidden name=q value=ice>".repeat(1025),
        format!("<input type=hidden name=q value='{}'>", "冰".repeat(1024)).repeat(130),
    ] {
        let mut browser = Browser::new(&format!("<form method=post action='/posted'>{fields}<button aria-label=Submit>Send</button></form>"));
        assert!(
            matches!(browser.click_reply("Submit"), ServerMessage::Error { message } if message.contains("limit"))
        );
        assert!(browser.requests.lock().unwrap().is_empty());
        assert!(browser.reviews.lock().unwrap().is_empty());
    }
}

#[test]
fn range_submission_matches_the_default_and_clamped_values_shown_by_the_core() {
    let mut browser = Browser::new("<form action='/received'><input type=range name=default aria-label=Default><input type=range name=limited aria-label=Limited min=10 max=20 value=99><input type=range name=bad aria-label=Invalid value=not-a-number><button aria-label=Submit>Send</button></form>");
    let snapshot = browser.snapshot();
    for (name, value) in [("Default", "50"), ("Limited", "20"), ("Invalid", "50")] {
        assert_eq!(
            snapshot
                .nodes
                .iter()
                .find(|node| node.name.as_deref() == Some(name))
                .unwrap()
                .state
                .value
                .as_deref(),
            Some(value)
        );
    }
    browser.submit("Submit");
    assert_eq!(
        browser.requests.lock().unwrap()[0].target,
        "/received?default=50&limited=20&bad=50"
    );
}
