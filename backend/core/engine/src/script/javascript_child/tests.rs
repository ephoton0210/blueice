// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::script::http_resource_authorizer::{
    sha256_integrity, HttpOutOfProcessPageScriptSourceAuthorizer, HttpScriptIntegrityManifest,
    HttpScriptResourceLimits, HttpScriptResourceOriginRule, HttpScriptResourcePolicy,
};
use crate::script::javascript::{AuthorizedJavaScriptModule, AuthorizedJavaScriptResolution};
use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution};
use blueice_ipc::script::ScriptReply;
use blueice_launcher::bluejs_host::{
    bind_bluejs_host_socket, serve_bluejs_host_listener, BlueJsChildHost,
};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
mod linked_pause;
mod private_scope;
mod scope_values;

fn unique_socket_path(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    PathBuf::from("/tmp").join(format!(
        "blueice-engine-child-host-{label}-{}-{count}.sock",
        std::process::id()
    ))
}

fn spawn_child() -> (PathBuf, String, thread::JoinHandle<()>) {
    let path = unique_socket_path("lifecycle");
    let token = "0123456789abcdef0123456789abcdef".to_string();
    let listener = bind_bluejs_host_socket(&path).unwrap();
    let child_token = token.clone();
    let handle = thread::spawn(move || {
        let mut host = BlueJsChildHost::default();
        serve_bluejs_host_listener(listener, child_token, &mut host).unwrap();
    });
    (path, token, handle)
}

fn loaded_tabs(html: &str, url: &str) -> (TabManager, TabId) {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab();
    tabs.get_mut(tab_id)
        .unwrap()
        .load_html_str(html, Some(url.to_string()));
    (tabs, tab_id)
}

#[test]
fn page_host_transport_pumps_before_the_child_replies() {
    let (core, mut child) = UnixStream::pair().unwrap();
    let (resume_sender, resume_receiver) = mpsc::sync_channel(1);
    let child_task = thread::spawn(move || {
        assert_eq!(
            page_host::read_page_host_request(&mut child).unwrap(),
            PageHostRequest::Shutdown
        );
        resume_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        page_host::write_page_host_reply(&mut child, &PageHostReply::ShutdownAck).unwrap();
    });
    let session_thread = thread::current().id();
    let mut pump_calls = 0;
    let reply = PageHostConnection { stream: core }
        .request_while_pumping_script(PageHostRequest::Shutdown, &mut || {
            assert_eq!(thread::current().id(), session_thread);
            pump_calls += 1;
            if pump_calls == 1 {
                resume_sender.send(()).unwrap();
            }
            Ok(())
        })
        .unwrap();
    child_task.join().unwrap();
    assert_eq!(reply, PageHostReply::ShutdownAck);
    assert!(pump_calls > 0);
}

#[test]
fn private_exception_location_wrapper_preserves_the_exact_child_tuple() {
    let (core, mut child) = UnixStream::pair().unwrap();
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 1 << 63,
        metadata_generation: 1 << 63,
    };
    let expected = PageHostReply::DebuggerBlueTsExceptionLocation {
        tab_id: 7,
        document_generation: 3,
        program,
        metadata,
        location: page_host::PageHostDebuggerBlueTsExceptionLocation {
            safe_point: PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: 1,
                bytecode_offset: 4,
            },
            span: page_host::PageHostDebuggerBlueTsSafePointSpan {
                source_id: 0,
                start_byte: 2,
                end_byte: 8,
                coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 2,
                    end_line: 0,
                    end_column_utf16: 8,
                },
            },
        },
    };
    let child_reply = expected.clone();
    let child_task = thread::spawn(move || {
        assert_eq!(
            page_host::read_page_host_request(&mut child).unwrap(),
            PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
                tab_id: 7,
                document_generation: 3,
                program,
                metadata,
            }
        );
        page_host::write_page_host_reply(&mut child, &child_reply).unwrap();
    });
    let mut connection = PageHostConnection { stream: core };
    assert!(connection.debugger_bluets_exception_location_available());
    assert_eq!(
        connection
            .debugger_bluets_exception_location(7, 3, program, metadata)
            .unwrap(),
        expected
    );
    child_task.join().unwrap();
}

#[test]
fn page_host_transport_times_out_and_poison_closes_a_stalled_child() {
    let (core, mut child) = UnixStream::pair().unwrap();
    let (release_sender, release_receiver) = mpsc::sync_channel(1);
    let child_task = thread::spawn(move || {
        assert_eq!(
            page_host::read_page_host_request(&mut child).unwrap(),
            PageHostRequest::Shutdown
        );
        release_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    });
    let mut connection = PageHostConnection { stream: core };
    let started = Instant::now();
    let error = connection
        .request_while_pumping_script_with_timeout(
            PageHostRequest::Shutdown,
            &mut || Ok(()),
            Duration::from_millis(100),
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(connection.request(PageHostRequest::Shutdown).is_err());
    release_sender.send(()).unwrap();
    child_task.join().unwrap();
}

#[test]
fn page_host_transport_fails_promptly_when_the_child_disconnects() {
    let (core, mut child) = UnixStream::pair().unwrap();
    let child_task = thread::spawn(move || {
        assert_eq!(
            page_host::read_page_host_request(&mut child).unwrap(),
            PageHostRequest::Shutdown
        );
    });
    let started = Instant::now();
    let error = PageHostConnection { stream: core }
        .request_while_pumping_script_with_timeout(
            PageHostRequest::Shutdown,
            &mut || Ok(()),
            Duration::from_secs(3),
        )
        .unwrap_err();
    child_task.join().unwrap();
    assert_ne!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn nested_child_wait_rejects_calls_after_its_total_budget() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<div id='target'>before</div>",
        "https://example.test/nested-budget.html",
    );
    let target = ScriptDocumentTarget {
        tab_id: tab_id.as_u64(),
        document_generation: tabs.get(tab_id).unwrap().document_generation(),
    };
    let (sender, receiver) = crate::script::script_request_channel();
    let first = thread::spawn({
        let sender = sender.clone();
        move || {
            sender.request(blueice_ipc::script::ScriptRequest::GetElementById {
                target,
                id: "target".to_string(),
            })
        }
    });
    let mut remaining = 1;
    let deadline = Instant::now() + Duration::from_secs(1);
    while remaining > 0 {
        pump_script_requests_during_child_wait(&receiver, &mut tabs, target, &mut remaining)
            .unwrap();
        assert!(
            Instant::now() < deadline,
            "first nested request did not arrive"
        );
        thread::yield_now();
    }
    let ScriptReply::Node { node: Some(node) } = first.join().unwrap().unwrap() else {
        panic!("the first request must resolve the target node");
    };
    pump_script_requests_during_child_wait(&receiver, &mut tabs, target, &mut remaining).unwrap();
    let before = tabs.get(tab_id).unwrap().dom_dump();
    let excess = thread::spawn(move || {
        sender.request(blueice_ipc::script::ScriptRequest::SetTextContent {
            target,
            node,
            value: "over budget".to_string(),
        })
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if pump_script_requests_during_child_wait(&receiver, &mut tabs, target, &mut remaining)
            .is_err()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "excess nested request did not arrive"
        );
        thread::yield_now();
    }
    assert!(matches!(
        excess.join().unwrap().unwrap(),
        ScriptReply::Error { .. }
    ));
    assert_eq!(tabs.get(tab_id).unwrap().dom_dump(), before);
}

struct ReentrantScriptChild {
    script_sender: crate::script::ScriptRequestSender,
}

fn nested_dom_write(
    sender: crate::script::ScriptRequestSender,
    target: ScriptDocumentTarget,
    value: &'static str,
    pump: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let (done_sender, done_receiver) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = (|| {
            let ScriptReply::Node { node: Some(node) } =
                sender.request(blueice_ipc::script::ScriptRequest::GetElementById {
                    target,
                    id: "target".to_string(),
                })?
            else {
                return Err(io::Error::other("target was not found"));
            };
            sender.request(blueice_ipc::script::ScriptRequest::SetTextContent {
                target,
                node,
                value: value.to_string(),
            })
        })();
        done_sender.send(result).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let script_reply = loop {
        pump()?;
        match done_receiver.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => break result?,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("script worker disconnected"));
            }
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "nested DOM call stalled",
                ));
            }
        }
    };
    worker.join().unwrap();
    assert_eq!(script_reply, ScriptReply::Ack);
    Ok(())
}

impl PageHostClient for ReentrantScriptChild {
    fn synchronize_document(&mut self, _document: PageHostDocument) -> io::Result<PageHostReply> {
        panic!("a session-owned child sync must use the nested script pump");
    }

    fn synchronize_document_with_script_pump(
        &mut self,
        document: PageHostDocument,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        let target = ScriptDocumentTarget {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
        };
        nested_dom_write(
            self.script_sender.clone(),
            target,
            "from nested script",
            pump,
        )?;
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn advance_debugger_execution_with_script_pump(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        pump: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<PageHostReply> {
        nested_dom_write(
            self.script_sender.clone(),
            ScriptDocumentTarget {
                tab_id,
                document_generation,
            },
            "from resumed script",
            pump,
        )?;
        Ok(PageHostReply::DebuggerExecutionAdvanced {
            tab_id,
            document_generation,
            reports: Vec::new(),
        })
    }
}

#[test]
fn child_document_sync_serves_dom_calls_on_the_owning_session_thread() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<div id='target'>before</div><script>let page = 1;</script>",
        "https://example.test/nested-dom.html",
    );
    let (script_sender, script_receiver) = crate::script::script_request_channel();
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::new(ReentrantScriptChild { script_sender });
    executor
        .synchronize_and_execute_serving_script(&mut tabs, &script_receiver)
        .unwrap();
    assert!(tabs
        .get(tab_id)
        .unwrap()
        .dom_dump()
        .contains("from nested script"));
    assert_eq!(executor.live_documents.len(), 1);
}

#[test]
fn debugger_resume_serves_dom_calls_on_the_owning_session_thread() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<div id='target'>before</div><script>let page = 1;</script>",
        "https://example.test/nested-debugger-dom.html",
    );
    let (script_sender, script_receiver) = crate::script::script_request_channel();
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        ReentrantScriptChild { script_sender },
    );
    executor
        .synchronize_and_execute_serving_script(&mut tabs, &script_receiver)
        .unwrap();
    executor
        .synchronize_and_execute_serving_script(&mut tabs, &script_receiver)
        .unwrap();
    assert!(tabs
        .get(tab_id)
        .unwrap()
        .dom_dump()
        .contains("from resumed script"));
    assert_eq!(executor.live_documents.len(), 1);
}

fn shutdown_child(path: &Path, token: &str) {
    let mut stream = UnixStream::connect(path).unwrap();
    page_host::write_page_host_request(
        &mut stream,
        &PageHostRequest::Hello {
            protocol_version: page_host::PAGE_HOST_PROTOCOL_VERSION,
            session_token: token.to_string(),
        },
    )
    .unwrap();
    assert!(matches!(
        page_host::read_page_host_reply(&mut stream).unwrap(),
        PageHostReply::HelloAck { .. }
    ));
    page_host::write_page_host_request(&mut stream, &PageHostRequest::Shutdown).unwrap();
    assert_eq!(
        page_host::read_page_host_reply(&mut stream).unwrap(),
        PageHostReply::ShutdownAck
    );
}

#[derive(Clone)]
struct HttpTestResponse {
    status: &'static str,
    content_type: Option<&'static str>,
    extra_headers: Vec<(&'static str, &'static str)>,
    body: String,
}

impl HttpTestResponse {
    fn script(content_type: &'static str, body: impl Into<String>) -> Self {
        Self {
            status: "200 OK",
            content_type: Some(content_type),
            extra_headers: Vec::new(),
            body: body.into(),
        }
    }

    fn redirect(location: &'static str) -> Self {
        Self {
            status: "302 Found",
            content_type: None,
            extra_headers: vec![("Location", location)],
            body: String::new(),
        }
    }

    fn write_to(&self, stream: &mut std::net::TcpStream) {
        let mut response = format!(
            "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.status,
            self.body.len()
        );
        if let Some(content_type) = self.content_type {
            response.push_str(&format!("Content-Type: {content_type}\r\n"));
        }
        for (name, value) in &self.extra_headers {
            response.push_str(&format!("{name}: {value}\r\n"));
        }
        response.push_str("\r\n");
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(self.body.as_bytes()).unwrap();
    }
}

fn spawn_local_resource_server(
    responses: BTreeMap<String, HttpTestResponse>,
    expected_requests: usize,
) -> (String, Arc<Mutex<Vec<String>>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let requested = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&requested);
    let thread = thread::spawn(move || {
        for _ in 0..expected_requests {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 4096];
            let bytes = stream.read(&mut buffer).unwrap();
            let request = std::str::from_utf8(&buffer[..bytes]).unwrap();
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap()
                .to_string();
            observed.lock().unwrap().push(path.clone());
            responses
                .get(&path)
                .unwrap_or(&HttpTestResponse {
                    status: "404 Not Found",
                    content_type: None,
                    extra_headers: Vec::new(),
                    body: String::new(),
                })
                .write_to(&mut stream);
        }
    });
    (origin, requested, thread)
}

fn external_javascript_graph() -> AuthorizedJavaScriptModuleGraph {
    AuthorizedJavaScriptModuleGraph::new(
        "https://cdn.example.test/assets/external.js",
        [AuthorizedJavaScriptModule::new(
            "https://cdn.example.test/assets/external.js",
            "globalThis.externalJavaScript = 41;",
        )
        .unwrap()],
        [],
        "core-external-javascript-policy-v1",
    )
    .unwrap()
}

fn external_bluets_graph() -> AuthorizedPageScriptGraph {
    let entry = "https://cdn.example.test/assets/external.ts";
    let dependency = "https://cdn.example.test/assets/answer.ts";
    let loader = AuthorizedModuleLoader::new(
        [
            AuthorizedModule::new(
                entry,
                "import { answer } from './answer.ts'; export const result: number = answer;",
            ),
            AuthorizedModule::new(dependency, "export const answer: number = 41;"),
        ],
        [AuthorizedModuleResolution::new(
            entry,
            "./answer.ts",
            dependency,
        )],
    )
    .unwrap();
    AuthorizedPageScriptGraph::new(entry, loader, "core-external-bluets-policy-v1").unwrap()
}

struct ExternalGraphsAuthorizer {
    requests: Arc<Mutex<Vec<OutOfProcessPageScriptSourceRequest>>>,
}

impl OutOfProcessPageScriptSourceAuthorizer for ExternalGraphsAuthorizer {
    fn authorize(
        &self,
        request: &OutOfProcessPageScriptSourceRequest,
    ) -> Result<AuthorizedOutOfProcessPageScriptGraph, OutOfProcessPageScriptSourceAuthorizationError>
    {
        self.requests.lock().unwrap().push(request.clone());
        match (
            request.document_generation,
            request.ordinal,
            request.language,
            request.document_url.as_str(),
            request.declared_src.as_str(),
        ) {
            (
                1,
                0,
                CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic),
                "https://example.test/app/index.html",
                "/assets/external.js",
            ) => Ok(AuthorizedOutOfProcessPageScriptGraph::JavaScript(
                external_javascript_graph(),
            )),
            (
                1,
                1,
                CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Module),
                "https://example.test/app/index.html",
                "/assets/external.ts",
            ) => Ok(AuthorizedOutOfProcessPageScriptGraph::BlueTs(
                external_bluets_graph(),
            )),
            _ => Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                "unexpected private test authorization request",
            )),
        }
    }
}

struct DeniedAndInvalidGraphsAuthorizer;

impl OutOfProcessPageScriptSourceAuthorizer for DeniedAndInvalidGraphsAuthorizer {
    fn authorize(
        &self,
        request: &OutOfProcessPageScriptSourceRequest,
    ) -> Result<AuthorizedOutOfProcessPageScriptGraph, OutOfProcessPageScriptSourceAuthorizationError>
    {
        match request.language {
            CombinedPageScriptLanguage::JavaScript(_) => {
                Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                    "private policy denied https://secret.example.test/denied.js",
                ))
            }
            CombinedPageScriptLanguage::BlueTs(_) => {
                Ok(AuthorizedOutOfProcessPageScriptGraph::BlueTs(
                    AuthorizedPageScriptGraph::new(
                        "https://secret.example.test/invalid.ts",
                        AuthorizedModuleLoader::new([], []).unwrap(),
                        "invalid-test-policy-v1",
                    )
                    .unwrap(),
                ))
            }
        }
    }
}

struct MissingStaticEdgeAuthorizer;

impl OutOfProcessPageScriptSourceAuthorizer for MissingStaticEdgeAuthorizer {
    fn authorize(
        &self,
        request: &OutOfProcessPageScriptSourceRequest,
    ) -> Result<AuthorizedOutOfProcessPageScriptGraph, OutOfProcessPageScriptSourceAuthorizationError>
    {
        if request.language != CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Module)
        {
            return Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                "unexpected private test authorization request",
            ));
        }
        Ok(AuthorizedOutOfProcessPageScriptGraph::JavaScript(
            AuthorizedJavaScriptModuleGraph::new(
                "https://cdn.example.test/assets/missing-edge.js",
                [AuthorizedJavaScriptModule::new(
                    "https://cdn.example.test/assets/missing-edge.js",
                    "import './not-authorized.js';",
                )
                .unwrap()],
                [],
                "core-no-fallback-policy-v1",
            )
            .unwrap(),
        ))
    }
}

struct NavigationAuthorizer {
    requests: Arc<Mutex<Vec<OutOfProcessPageScriptSourceRequest>>>,
}

impl OutOfProcessPageScriptSourceAuthorizer for NavigationAuthorizer {
    fn authorize(
        &self,
        request: &OutOfProcessPageScriptSourceRequest,
    ) -> Result<AuthorizedOutOfProcessPageScriptGraph, OutOfProcessPageScriptSourceAuthorizationError>
    {
        self.requests.lock().unwrap().push(request.clone());
        let (document_url, entry, source, fingerprint) = match request.document_generation {
            1 => (
                "https://first.example.test/one",
                "https://cdn.example.test/assets/first.js",
                "globalThis.firstExternalGeneration = 1;",
                "core-navigation-policy-one-v1",
            ),
            2 => (
                "https://second.example.test/two",
                "https://cdn.example.test/assets/second.js",
                concat!(
                    "if (typeof globalThis.firstExternalGeneration !== 'undefined') ",
                    "throw new Error('stale realm');",
                    "globalThis.secondExternalGeneration = 2;"
                ),
                "core-navigation-policy-two-v1",
            ),
            _ => {
                return Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                    "unexpected private test document generation",
                ));
            }
        };
        if request.ordinal != 0
            || request.language
                != CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic)
            || request.document_url != document_url
            || request.declared_src != "/assets/navigation.js"
        {
            return Err(OutOfProcessPageScriptSourceAuthorizationError::new(
                "unexpected private test authorization request",
            ));
        }
        Ok(AuthorizedOutOfProcessPageScriptGraph::JavaScript(
            AuthorizedJavaScriptModuleGraph::new(
                entry,
                [AuthorizedJavaScriptModule::new(entry, source).unwrap()],
                Vec::<AuthorizedJavaScriptResolution>::new(),
                fingerprint,
            )
            .unwrap(),
        ))
    }
}

#[derive(Default)]
struct RecordingChild {
    documents: Vec<PageHostDocument>,
    closes: Vec<(u64, u64)>,
    child_stats_reply: Option<PageHostReply>,
}

impl PageHostClient for RecordingChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        let reply = PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        };
        self.documents.push(document);
        Ok(reply)
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.closes.push((tab_id, document_generation));
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmStats(page_host::PageHostRealmStats {
            tab_id,
            document_generation,
            program_count: 1,
            bytecode_bytes: 64,
            heap_bytes: 128,
        }))
    }

    fn child_stats(&mut self) -> io::Result<PageHostReply> {
        self.child_stats_reply.clone().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "child-wide accounting unavailable",
            )
        })
    }
}

#[test]
fn core_accepts_only_well_formed_child_wide_usage_for_its_live_realm_count() {
    let (tabs, _tab_id) = loaded_tabs(
        "<script>let accounting = 1;</script>",
        "https://example.test/aggregate-accounting.html",
    );
    let valid = PageHostChildStats {
        realm_count: 1,
        program_count: 1,
        bytecode_bytes: 64,
        heap_bytes: 128,
    };
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild {
        child_stats_reply: Some(PageHostReply::ChildStats(valid)),
        ..RecordingChild::default()
    });
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.child_stats().unwrap(), valid);
    executor.child.child_stats_reply = Some(PageHostReply::ChildStats(PageHostChildStats {
        realm_count: 2,
        ..valid
    }));
    assert_eq!(
        executor.child_stats().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    executor.child.child_stats_reply = Some(PageHostReply::ChildStats(PageHostChildStats {
        program_count: u64::MAX,
        ..valid
    }));
    assert_eq!(
        executor.child_stats().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    executor.child.child_stats_reply = Some(PageHostReply::RealmStats(PageHostRealmStats {
        tab_id: 1,
        document_generation: 1,
        program_count: 1,
        bytecode_bytes: 64,
        heap_bytes: 128,
    }));
    assert_eq!(
        executor.child_stats().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn core_caches_child_realm_accounting_only_for_the_live_generation() {
    let (mut tabs, tab_id) = loaded_tabs(
        "<script>let accounting = 1;</script>",
        "https://example.test/accounting-first.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.realm_stats(tab_id),
        Some(&PageHostRealmStats {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            program_count: 1,
            bytecode_bytes: 64,
            heap_bytes: 128,
        })
    );

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>let accounting = 2;</script>",
        Some("https://example.test/accounting-successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .realm_stats(tab_id)
            .map(|stats| stats.document_generation),
        Some(2),
        "a replacement must not retain the predecessor accounting record"
    );

    assert!(tabs.close_tab(tab_id));
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.realm_stats(tab_id), None);
    assert_eq!(executor.into_child().closes, vec![(tab_id.as_u64(), 2)]);
}

#[derive(Clone, Copy, Default)]
enum InvalidRealmStats {
    #[default]
    MismatchedTuple,
    ExcessPrograms,
    SaturatedBytecode,
    SaturatedHeap,
}

#[derive(Default)]
struct InvalidStatsChild {
    closes: Vec<(u64, u64)>,
    invalid_stats: InvalidRealmStats,
}

impl PageHostClient for InvalidStatsChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.closes.push((tab_id, document_generation));
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        let stats = match self.invalid_stats {
            InvalidRealmStats::MismatchedTuple => page_host::PageHostRealmStats {
                tab_id: tab_id.saturating_add(1),
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
            },
            InvalidRealmStats::ExcessPrograms => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: page_host::PAGE_HOST_REALM_STATS_MAX_PROGRAMS + 1,
                bytecode_bytes: 64,
                heap_bytes: 128,
            },
            InvalidRealmStats::SaturatedBytecode => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: u64::MAX,
                heap_bytes: 128,
            },
            InvalidRealmStats::SaturatedHeap => page_host::PageHostRealmStats {
                tab_id,
                document_generation,
                program_count: 1,
                bytecode_bytes: 64,
                heap_bytes: u64::MAX,
            },
        };
        Ok(PageHostReply::RealmStats(stats))
    }
}

#[test]
fn malformed_child_realm_accounting_is_never_cached() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let untrustedAccounting = 1;</script>",
        "https://example.test/mismatched-accounting.html",
    );
    for invalid_stats in [
        InvalidRealmStats::MismatchedTuple,
        InvalidRealmStats::ExcessPrograms,
        InvalidRealmStats::SaturatedBytecode,
        InvalidRealmStats::SaturatedHeap,
    ] {
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(InvalidStatsChild {
            invalid_stats,
            ..InvalidStatsChild::default()
        });
        executor.synchronize_and_execute(&tabs).unwrap();

        assert_eq!(executor.realm_stats(tab_id), None);
        assert!(
            !executor.debugger_has_live_realm(tab_id, 1),
            "malformed accounting must not be accepted as debugger liveness"
        );
        assert_eq!(
            executor.into_child().closes,
            vec![(tab_id.as_u64(), 1)],
            "an untrustworthy accounting record must close the newly acknowledged realm"
        );
    }
}

#[derive(Default)]
struct WrongSuccessorAckChild {
    active_generation: Option<u64>,
    closes: Vec<(u64, u64)>,
    return_error: bool,
}

impl PageHostClient for WrongSuccessorAckChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.active_generation = Some(document.document_generation);
        if document.document_generation == 2 && self.return_error {
            return Ok(PageHostReply::Error {
                code: PageHostErrorCode::HostFailure,
                message: "child failed after admission".to_string(),
            });
        }
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id + u64::from(document.document_generation == 2),
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.closes.push((tab_id, document_generation));
        if self.active_generation == Some(document_generation) {
            self.active_generation = None;
            Ok(PageHostReply::RealmClosed {
                tab_id,
                document_generation,
            })
        } else {
            Ok(PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                message: "stale document".to_string(),
            })
        }
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        if self.active_generation != Some(document_generation) {
            return Ok(PageHostReply::Error {
                code: PageHostErrorCode::StaleDocument,
                message: "stale document".to_string(),
            });
        }
        Ok(PageHostReply::RealmStats(PageHostRealmStats {
            tab_id,
            document_generation,
            program_count: 1,
            bytecode_bytes: 64,
            heap_bytes: 128,
        }))
    }

    fn child_stats(&mut self) -> io::Result<PageHostReply> {
        let has_realm = self.active_generation.is_some();
        Ok(PageHostReply::ChildStats(PageHostChildStats {
            realm_count: u32::from(has_realm),
            program_count: u64::from(has_realm),
            bytecode_bytes: if has_realm { 64 } else { 0 },
            heap_bytes: if has_realm { 128 } else { 0 },
        }))
    }
}

#[test]
fn untrusted_successor_ack_closes_the_generation_the_child_may_have_admitted() {
    for return_error in [false, true] {
        let (mut tabs, tab_id) = loaded_tabs(
            "<script>let previous = 1;</script>",
            "https://example.test/previous.html",
        );
        let mut executor = OutOfProcessJavaScriptPageExecutor::new(WrongSuccessorAckChild {
            return_error,
            ..WrongSuccessorAckChild::default()
        });
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.child.active_generation, Some(1));
        assert_eq!(executor.child_stats().unwrap().realm_count, 1);

        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<script>let successor = 2;</script>",
            Some("https://example.test/successor.html".to_string()),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(executor.child_stats().unwrap().realm_count, 0);
        let child = executor.into_child();
        assert_eq!(child.active_generation, None);
        assert_eq!(
            child.closes,
            vec![(tab_id.as_u64(), 1), (tab_id.as_u64(), 2)]
        );
    }
}

/// Records only the lifecycle advance requests needed to prove that a
/// debugger discovery peer receives a finite grace budget rather than an
/// execution lease. It deliberately implements no debugger inspection
/// operation, so no VM/source/bytecode surface is added by this test.
#[derive(Default)]
struct DeferralBudgetChild {
    advances: Vec<(u64, u64)>,
}

impl PageHostClient for DeferralBudgetChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn advance_debugger_execution(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.advances.push((tab_id, document_generation));
        Ok(PageHostReply::DebuggerExecutionAdvanced {
            tab_id,
            document_generation,
            reports: Vec::new(),
        })
    }
}

#[test]
fn oop_debugger_discovery_deferrals_are_finite_per_document() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let pendingDebuggerAdmission = true;</script>",
        "https://example.test/pending-debugger.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        DeferralBudgetChild::default(),
    );
    executor.synchronize_and_execute(&tabs).unwrap();

    // One hold is accepted for each of the fixed number of session turns,
    // matching a debugger peer that asks one discovery/configuration
    // question per core tick. The next request cannot keep the document
    // pending; core advances it through the child lifecycle instead.
    for _ in 0..MAX_OOP_DEBUGGER_EXECUTION_DEFERRALS_PER_DOCUMENT {
        executor.hold_pending_debugger_execution_once();
        executor.synchronize_and_execute(&tabs).unwrap();
    }
    assert!(executor.child.advances.is_empty());

    executor.hold_pending_debugger_execution_once();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(executor.child.advances, vec![(tab_id.as_u64(), 1)]);
}

#[test]
fn immutable_core_authorizer_routes_external_javascript_and_bluets_graphs_to_real_child() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script src=\"/assets/external.js\"></script>",
                "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/external.ts\"></script>",
                "<script>if (globalThis.externalJavaScript !== 41) throw new Error('order');</script>"
            ),
            "https://example.test/app/index.html",
        );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path,
        &token,
        ExternalGraphsAuthorizer {
            requests: Arc::clone(&requests),
        },
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 2,
                kind: BlueJsPageScriptKind::Classic,
            },
        ]
    );
    assert_eq!(
        executor.drain_blue_ts_reports_for_tab(tab_id),
        vec![BlueTsPageExecutionReport::Executed {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 1,
            kind: DirectPageScriptKind::Module,
        }]
    );
    assert_eq!(
        requests.lock().unwrap().as_slice(),
        [
            OutOfProcessPageScriptSourceRequest {
                tab_id,
                document_generation: 1,
                ordinal: 0,
                language: CombinedPageScriptLanguage::JavaScript(BlueJsPageScriptKind::Classic,),
                document_url: "https://example.test/app/index.html".to_string(),
                declared_src: "/assets/external.js".to_string(),
            },
            OutOfProcessPageScriptSourceRequest {
                tab_id,
                document_generation: 1,
                ordinal: 1,
                language: CombinedPageScriptLanguage::BlueTs(DirectPageScriptKind::Module),
                document_url: "https://example.test/app/index.html".to_string(),
                declared_src: "/assets/external.ts".to_string(),
            },
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn startup_http_authorizer_builds_closed_classic_module_and_bluets_graphs_source_free() {
    let classic = "globalThis.externalClassic = (globalThis.externalClassic || 40) + 1;";
    let javascript_entry =
        "import { answer } from './answer.js'; globalThis.externalModule = answer;";
    let javascript_dependency = "export const answer = 42;";
    let bluets_entry =
        "import { answer } from './answer.ts'; export const result: number = answer;";
    let bluets_dependency = "export const answer: number = 41;";
    let integrity_mismatch = "globalThis.redactedIntegrityMarker = 'private bytes';";
    let bad_mime = "globalThis.redactedMimeMarker = 'private bytes';";
    let mut responses = BTreeMap::new();
    responses.insert(
        "/assets/classic.js".to_string(),
        HttpTestResponse::script("text/javascript; charset=utf-8", classic),
    );
    responses.insert(
        "/assets/main.js".to_string(),
        HttpTestResponse::script("application/javascript", javascript_entry),
    );
    responses.insert(
        "/assets/answer.js".to_string(),
        HttpTestResponse::script("application/javascript", javascript_dependency),
    );
    responses.insert(
        "/assets/main.ts".to_string(),
        HttpTestResponse::script("text/typescript", bluets_entry),
    );
    responses.insert(
        "/assets/answer.ts".to_string(),
        HttpTestResponse::script("application/typescript", bluets_dependency),
    );
    responses.insert(
        "/assets/bad-integrity.js".to_string(),
        HttpTestResponse::script("application/javascript", integrity_mismatch),
    );
    responses.insert(
        "/assets/bad-mime.js".to_string(),
        HttpTestResponse::script("text/plain", bad_mime),
    );
    responses.insert(
        "/assets/redirect.js".to_string(),
        HttpTestResponse::redirect("/assets/classic.js"),
    );
    // Eight requests: the repeated classic declaration uses the private
    // `(URL, integrity, MIME lane)` cache key, and the cross-origin URL
    // is rejected before any network I/O.
    let (origin, requested, server) = spawn_local_resource_server(responses, 8);
    let resource = |path: &str| format!("{origin}{path}");
    let manifest = HttpScriptIntegrityManifest::new([
        (
            resource("/assets/classic.js"),
            sha256_integrity(classic.as_bytes()),
        ),
        (
            resource("/assets/main.js"),
            sha256_integrity(javascript_entry.as_bytes()),
        ),
        (
            resource("/assets/answer.js"),
            sha256_integrity(javascript_dependency.as_bytes()),
        ),
        (
            resource("/assets/main.ts"),
            sha256_integrity(bluets_entry.as_bytes()),
        ),
        (
            resource("/assets/answer.ts"),
            sha256_integrity(bluets_dependency.as_bytes()),
        ),
        (
            resource("/assets/bad-integrity.js"),
            sha256_integrity(b"owner-selected different bytes"),
        ),
        (
            resource("/assets/bad-mime.js"),
            sha256_integrity(bad_mime.as_bytes()),
        ),
        (
            resource("/assets/redirect.js"),
            sha256_integrity(b"redirect body is not admitted"),
        ),
        (
            "http://127.0.0.1:1/cross-origin.js".to_string(),
            sha256_integrity(b"not fetched"),
        ),
    ])
    .unwrap();
    let policy = HttpScriptResourcePolicy::new(
        HttpScriptResourceOriginRule::same_document_origin(),
        manifest,
        HttpScriptResourceLimits::default(),
    )
    .unwrap();
    let authorizer = HttpOutOfProcessPageScriptSourceAuthorizer::new(policy);
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script src=\"/assets/classic.js\"></script>",
                "<script type=\"module\" src=\"/assets/main.js\"></script>",
                "<script type=\"application/x-blueice-typescript-module\" src=\"/assets/main.ts\"></script>",
                "<script src=\"/assets/classic.js\"></script>",
                "<script src=\"/assets/bad-integrity.js\"></script>",
                "<script type=\"module\" src=\"/assets/bad-mime.js\"></script>",
                "<script src=\"/assets/redirect.js\"></script>",
                "<script src=\"http://127.0.0.1:1/cross-origin.js\"></script>",
                "<script>if (globalThis.externalClassic !== 42 || globalThis.externalModule !== 42) throw 'graph';</script>"
            ),
            &format!("{origin}/app/index.html"),
        );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path, &token, authorizer,
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    let reports = executor.drain_reports_for_tab(tab_id);
    assert_eq!(
        reports,
        vec![
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: BlueJsPageScriptKind::Module,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 3,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 4,
                kind: BlueJsPageScriptKind::Classic,
                category: "external JavaScript source authorization rejected the page script",
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 5,
                kind: BlueJsPageScriptKind::Module,
                category: "external JavaScript source authorization rejected the page script",
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 6,
                kind: BlueJsPageScriptKind::Classic,
                category: "external JavaScript source authorization rejected the page script",
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 7,
                kind: BlueJsPageScriptKind::Classic,
                category: "external JavaScript source authorization rejected the page script",
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 8,
                kind: BlueJsPageScriptKind::Classic,
            },
        ]
    );
    assert_eq!(
        executor.drain_blue_ts_reports_for_tab(tab_id),
        vec![BlueTsPageExecutionReport::Executed {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 2,
            kind: DirectPageScriptKind::Module,
        }]
    );
    let source_free = format!("{reports:?}");
    for protected in [
        "redactedIntegrityMarker",
        "redactedMimeMarker",
        "bad-integrity.js",
        "bad-mime.js",
        "redirect.js",
        "cross-origin.js",
    ] {
        assert!(!source_free.contains(protected));
    }
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
    server.join().unwrap();
    let requested = requested.lock().unwrap();
    assert_eq!(requested.len(), 8);
    assert_eq!(
        requested
            .iter()
            .filter(|path| path.as_str() == "/assets/classic.js")
            .count(),
        1
    );
    assert!(!requested.iter().any(|path| path.contains("cross-origin")));
}

#[test]
fn external_authorizer_denial_and_invalid_graph_are_source_free_in_real_child_route() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script src=\"https://secret.example.test/denied.js\"></script>",
                "<script type=\"application/x-blueice-typescript\" src=\"https://secret.example.test/invalid.ts\"></script>"
            ),
            "https://example.test/app/index.html",
        );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path,
        &token,
        DeniedAndInvalidGraphsAuthorizer,
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    let reports = executor.drain_reports_for_tab(tab_id);
    let blue_ts_reports = executor.drain_blue_ts_reports_for_tab(tab_id);
    assert!(matches!(
        reports.as_slice(),
        [JavaScriptPageExecutionReport::Rejected {
            category: "external JavaScript source authorization rejected the page script",
            ..
        }]
    ));
    assert!(matches!(
        blue_ts_reports.as_slice(),
        [BlueTsPageExecutionReport::Rejected {
            category: "external BlueTS source authorization rejected the page script",
            ..
        }]
    ));
    let observed = format!("{reports:?}{blue_ts_reports:?}");
    assert!(!observed.contains("secret.example.test"));
    assert!(!observed.contains("invalid.ts"));
    assert!(!observed.contains("private policy"));
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn missing_authorized_static_edge_has_no_child_resolver_fallback() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<script type=\"module\" src=\"/assets/missing-edge.js\"></script>",
            "<script>globalThis.afterMissingEdge = true;</script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path,
        &token,
        MissingStaticEdgeAuthorizer,
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();

    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Module,
                category: "authorized JavaScript graph is missing a static resolution",
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: BlueJsPageScriptKind::Classic,
            },
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn external_authorization_is_rechecked_and_old_child_realm_is_invalidated_on_navigation() {
    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
        "<script src=\"/assets/navigation.js\"></script>",
        "https://first.example.test/one",
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect_with_external_source_authorizer(
        &path,
        &token,
        NavigationAuthorizer {
            requests: Arc::clone(&requests),
        },
    )
    .unwrap();

    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            document_generation: 1,
            ordinal: 0,
            ..
        }]
    ));
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script src=\"/assets/navigation.js\"></script>",
        Some("https://second.example.test/two".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            document_generation: 2,
            ordinal: 0,
            ..
        }]
    ));
    assert_eq!(
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| (
                request.document_generation,
                request.ordinal,
                request.document_url.as_str(),
                request.declared_src.as_str(),
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                1,
                0,
                "https://first.example.test/one",
                "/assets/navigation.js"
            ),
            (
                2,
                0,
                "https://second.example.test/two",
                "/assets/navigation.js"
            ),
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn core_routes_explicit_inline_documents_to_the_real_child_and_rejects_external_src() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<script>globalThis.answer = 42;</script>",
            "<script type=\"module\">export const moduleAnswer = 43;</script>",
            "<script src=\"untrusted.js\"></script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let stats = executor
        .realm_stats(tab_id)
        .expect("the real child must supply core-owned realm accounting");
    assert_eq!(stats.tab_id, tab_id.as_u64());
    assert_eq!(stats.document_generation, 1);
    assert_eq!(stats.program_count, 2);
    assert!(
        stats.bytecode_bytes > 0,
        "the aggregate record must charge the admitted classic and module programs"
    );
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: BlueJsPageScriptKind::Module,
            },
            JavaScriptPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 2,
                kind: BlueJsPageScriptKind::Classic,
                category: "external JavaScript declarations require an authorized loader",
            },
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn core_proxies_real_child_debugger_locations_with_core_ids_and_rejects_stale_cross_tab_targets() {
    use crate::debugger::handle_debugger_request_with_page_javascript_executor;
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerErrorCode, DebuggerPageRealm,
        DebuggerReply, DebuggerRequest,
    };

    let (path, token, child) = spawn_child();
    let (mut tabs, first_tab) = loaded_tabs(
        "<script>let first = 1; first += 1;</script>",
        "https://example.test/first.html",
    );
    let second_tab = tabs.open_tab();
    tabs.get_mut(second_tab).unwrap().load_html_str(
        "<script>let second = 2;</script>",
        Some("https://example.test/second.html".to_string()),
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();

    let first_realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: first_tab.as_u64(),
        realm_generation: 1,
    };
    let second_realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: second_tab.as_u64(),
        realm_generation: 1,
    };
    let capabilities = handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::DescribeCapabilities { realm: first_realm },
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        panic!("expected child debugger capabilities");
    };
    assert_eq!(
        capabilities
            .reports
            .iter()
            .find(|report| report.capability == DebuggerCapability::ProgramLocations)
            .map(|report| report.state),
        Some(DebuggerCapabilityState::Available)
    );
    assert_eq!(
        capabilities
            .reports
            .iter()
            .find(|report| report.capability == DebuggerCapability::BreakpointConfiguration)
            .map(|report| report.state),
        Some(DebuggerCapabilityState::Available)
    );
    assert!(
        capabilities
            .reports
            .iter()
            .all(|report| !report.detail.contains("first") && !report.detail.contains("bytecode")),
        "capability replies must remain source/bytecode-free"
    );

    let programs = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm: first_realm },
    ) {
        DebuggerReply::Programs(programs) => programs,
        reply => panic!("expected public program inventory, got {reply:?}"),
    };
    let program = *programs.first().expect("first child page has one program");
    assert!(
        program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
            && program.program_generation >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
        "core must mint a public namespace instead of forwarding child IDs"
    );
    let safe_points = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) {
        DebuggerReply::SafePoints(safe_points) => safe_points,
        reply => panic!("expected source-free public safe points, got {reply:?}"),
    };
    let safe_point = *safe_points.first().expect("program exposes one safe point");
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ValidateSafePoint { safe_point },
        ),
        DebuggerReply::SafePointValidated { safe_point }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );
    // Configuration is idempotent and does not create another public or
    // child-private record on a debugger socket retry.
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListBreakpoints { realm: first_realm },
        ),
        DebuggerReply::Breakpoints(vec![safe_point])
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ClearBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: true,
        }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ClearBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: false,
        }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );

    let cross_tab = blueice_ipc::debugger::DebuggerProgram {
        realm: second_realm,
        ..program
    };
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints { program: cross_tab },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let cross_tab_safe_point = blueice_ipc::debugger::DebuggerSafePoint {
        program: cross_tab,
        ..safe_point
    };
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint {
                safe_point: cross_tab_safe_point,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));

    // Reconfiguration before navigation proves that the successor's
    // empty child table cannot retain prior private IDs or public tuples.
    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<script>let successor = 4;</script>",
        Some("https://example.test/successor-again.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let successor_realm = DebuggerPageRealm {
        realm_generation: 2,
        ..first_realm
    };
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListBreakpoints {
                realm: successor_realm,
            },
        ),
        DebuggerReply::Breakpoints(Vec::new())
    );

    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<script>let successor = 3;</script>",
        Some("https://example.test/successor.html".to_string()),
    );
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints { program },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));

    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn core_remints_real_child_bluets_metadata_handles_and_discards_them_on_navigation() {
    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
        concat!(
            "<script>globalThis.javaScriptOnly = true;</script>",
            "<script type=\"application/x-blueice-typescript\">",
            "const opaqueCompilerMetadata: number = 42;",
            "</script>"
        ),
        "https://example.test/opaque-metadata.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();

    let programs = executor.debugger_programs(tab_id, 1).unwrap();
    assert_eq!(programs.len(), 2);
    let mut metadata = None;
    for program in programs {
        let handles = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap();
        if let [handle] = handles.as_slice() {
            assert!(
                handle.metadata_handle >= CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
                    && handle.metadata_handle < CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
                    && handle.metadata_generation
                        >= CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
                    && handle.metadata_generation < CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
                "core must remint metadata IDs outside both child and public program namespaces"
            );
            metadata = Some((program, *handle));
        } else {
            assert!(handles.is_empty(), "the JavaScript program is ineligible");
        }
    }
    let (typed_program, metadata) = metadata.expect("direct BlueTS has one private attachment");
    assert!(
        !format!("{metadata:?}").contains("opaqueCompilerMetadata"),
        "the core-facing handle must contain no compiler metadata payload"
    );
    let summary = executor
        .debugger_static_metadata_summary(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        )
        .expect("the exact core-reminted metadata identity resolves a bounded summary");
    assert_eq!(summary.language_version, "blue-ts-0.1");
    assert!(summary.source_count > 0);
    assert!(summary.type_count > 0);
    assert!(summary.symbol_count > 0);
    assert!(
        !format!("{summary:?}").contains("opaqueCompilerMetadata"),
        "the core-facing summary must contain no compiler record payload"
    );
    let sources = executor
        .debugger_static_metadata_sources(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        )
        .expect("the exact core-reminted metadata identity resolves source-record IDs");
    assert_eq!(
        sources.len(),
        usize::try_from(summary.source_count).unwrap()
    );
    assert_eq!(
        sources
            .iter()
            .map(|source| source.source_id)
            .collect::<BTreeSet<_>>()
            .len(),
        sources.len(),
        "the child must not repeat compiler source-record IDs"
    );
    assert!(
        !format!("{sources:?}").contains("opaqueCompilerMetadata"),
        "source-record identities must not carry compiler record payloads"
    );
    assert!(matches!(
        executor.debugger_static_metadata_sources(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation + 1,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    ));
    assert!(matches!(
        executor.debugger_static_metadata_summary(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation + 1,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    ));

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">const successor: number = 1;</script>",
        Some("https://example.test/opaque-metadata-successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.debugger_static_metadata(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));
    assert!(matches!(
        executor.debugger_static_metadata_summary(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));
    assert!(matches!(
        executor.debugger_static_metadata_sources(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));

    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn core_child_transport_resolves_only_live_exact_bluets_safe_point_spans() {
    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const mapped: number = 42;</script>",
        "https://example.test/mapped.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let child_stats = executor.child_stats().unwrap();
    assert_eq!(child_stats.realm_count, 1);
    assert!(child_stats.is_well_formed());
    assert!(executor.child.debugger_bluets_safe_point_span_available());

    let PageHostReply::DebuggerPrograms { programs, .. } = executor
        .child
        .debugger_programs(tab_id.as_u64(), 1)
        .unwrap()
    else {
        panic!("expected a child-private BlueTS program");
    };
    let program = programs[0];
    let PageHostReply::DebuggerBlueTsMetadata { metadata, .. } = executor
        .child
        .debugger_bluets_metadata(tab_id.as_u64(), 1, program)
        .unwrap()
    else {
        panic!("expected a child-private metadata attachment");
    };
    let metadata = metadata[0];
    let PageHostReply::DebuggerSafePoints { safe_points, .. } = executor
        .child
        .debugger_safe_points(tab_id.as_u64(), 1, program)
        .unwrap()
    else {
        panic!("expected child-private safe points");
    };
    let (safe_point, span) = safe_points
        .into_iter()
        .find_map(|safe_point| {
            match executor
                .child
                .debugger_bluets_safe_point_span(tab_id.as_u64(), 1, metadata, safe_point)
                .unwrap()
            {
                PageHostReply::DebuggerBlueTsSafePointSpan {
                    safe_point: echoed,
                    span,
                    ..
                } if echoed == safe_point => Some((safe_point, span)),
                PageHostReply::Error {
                    code: page_host::PageHostErrorCode::InvalidRequest,
                    ..
                } => None,
                reply => panic!("unexpected exact-span reply: {reply:?}"),
            }
        })
        .expect("one verified safe point must have a retained BlueTS span");
    assert!(span.start_byte < span.end_byte);
    assert!(!format!("{span:?}").contains("mapped"));

    let public_program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let public_metadata = executor
        .debugger_static_metadata(
            tab_id,
            1,
            public_program.program_handle,
            public_program.program_generation,
        )
        .unwrap()[0];
    let public_sources = executor
        .debugger_static_metadata_sources(
            tab_id,
            1,
            public_program.program_handle,
            public_program.program_generation,
            public_metadata.metadata_handle,
            public_metadata.metadata_generation,
        )
        .unwrap();
    assert!(public_sources
        .iter()
        .any(|source| source.source_id == span.source_id));
    let target = JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
        program_handle: public_program.program_handle,
        program_generation: public_program.program_generation,
        metadata_handle: public_metadata.metadata_handle,
        metadata_generation: public_metadata.metadata_generation,
        source_id: span.source_id,
        code_unit_ordinal: safe_point.code_unit_ordinal,
        bytecode_offset: safe_point.bytecode_offset,
    };
    assert_eq!(
        executor
            .debugger_static_metadata_safe_point_span(tab_id, 1, target)
            .unwrap(),
        JavaScriptPageDebuggerStaticMetadataSafePointSpan {
            source_id: span.source_id,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            coordinates: span.coordinates,
        }
    );
    assert!(executor.debugger_static_metadata_source_breakpoint_available());
    let breakpoint_target = JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
        program_handle: public_program.program_handle,
        program_generation: public_program.program_generation,
        metadata_handle: public_metadata.metadata_handle,
        metadata_generation: public_metadata.metadata_generation,
        source_id: span.source_id,
        source_byte: span.start_byte,
    };
    assert_eq!(
        executor
            .debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target)
            .unwrap(),
        Some(JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: safe_point.code_unit_ordinal,
            bytecode_offset: safe_point.bytecode_offset,
        })
    );
    assert_eq!(
        executor
            .debugger_static_metadata_source_breakpoint(
                tab_id,
                1,
                JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
                    source_byte: span.end_byte,
                    ..breakpoint_target
                },
            )
            .unwrap(),
        None,
    );
    assert_eq!(
        executor.debugger_static_metadata_source_breakpoint(
            tab_id,
            1,
            JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
                metadata_generation: breakpoint_target.metadata_generation + 1,
                ..breakpoint_target
            },
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(
            tab_id,
            1,
            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                source_id: u32::MAX,
                ..target
            },
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(
            tab_id,
            1,
            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                metadata_generation: target.metadata_generation + 1,
                ..target
            },
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(
            tab_id,
            1,
            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                bytecode_offset: u32::MAX,
                ..target
            },
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>const successor = true;</script>",
        Some("https://example.test/successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        executor.debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert!(matches!(
        executor
            .child
            .debugger_bluets_safe_point_span(tab_id.as_u64(), 1, metadata, safe_point)
            .unwrap(),
        PageHostReply::Error {
            code: page_host::PageHostErrorCode::StaleDocument,
            ..
        }
    ));

    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

struct MalformedSafePointSpanChild {
    lifecycle: RecordingChild,
    reply: PageHostReply,
}

struct ExceptionLocationChild {
    lifecycle: RecordingChild,
    exception_reply: PageHostReply,
    point_reply: PageHostReply,
    span_reply: PageHostReply,
}

impl PageHostClient for ExceptionLocationChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.lifecycle.synchronize_document(document)
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.lifecycle.close_realm(tab_id, document_generation)
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.lifecycle
            .debugger_realm_stats(tab_id, document_generation)
    }

    fn debugger_bluets_metadata_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_sources_available(&self) -> bool {
        true
    }

    fn debugger_bluets_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_bluets_exception_location_available(&self) -> bool {
        true
    }

    fn debugger_bluets_exception_location(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
    ) -> io::Result<PageHostReply> {
        Ok(self.exception_reply.clone())
    }

    fn validate_debugger_safe_point(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(self.point_reply.clone())
    }

    fn debugger_bluets_safe_point_span(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _metadata: PageHostDebuggerMetadataHandle,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(self.span_reply.clone())
    }
}

#[test]
fn core_exception_adapter_remints_only_a_revalidated_live_child_location() {
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const typed: number = 1;</script>",
        "https://example.test/exception-adapter.html",
    );
    let child_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let child_metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let child_point = PageHostDebuggerSafePoint {
        program: child_program,
        code_unit_ordinal: 1,
        bytecode_offset: 4,
    };
    let span = page_host::PageHostDebuggerBlueTsSafePointSpan {
        source_id: 3,
        start_byte: 2,
        end_byte: 8,
        coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 2,
            end_line: 0,
            end_column_utf16: 8,
        },
    };
    let exception_reply = PageHostReply::DebuggerBlueTsExceptionLocation {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        program: child_program,
        metadata: child_metadata,
        location: page_host::PageHostDebuggerBlueTsExceptionLocation {
            safe_point: child_point,
            span,
        },
    };
    let point_reply = PageHostReply::DebuggerSafePointValidated {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        safe_point: child_point,
    };
    let span_reply = PageHostReply::DebuggerBlueTsSafePointSpan {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        metadata: child_metadata,
        safe_point: child_point,
        span,
    };
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        ExceptionLocationChild {
            lifecycle: RecordingChild::default(),
            exception_reply: exception_reply.clone(),
            point_reply: point_reply.clone(),
            span_reply: span_reply.clone(),
        },
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    executor.debugger_programs.insert(
        tab_id,
        BTreeMap::from([(
            child_program,
            CoreDebuggerProgram {
                program_handle: 101,
                program_generation: 103,
            },
        )]),
    );
    executor.debugger_static_metadata.insert(
        tab_id,
        BTreeMap::from([(
            child_metadata,
            CoreDebuggerStaticMetadata {
                program: child_program,
                metadata_handle: 107,
                metadata_generation: 109,
            },
        )]),
    );
    let target = JavaScriptPageDebuggerExceptionLocationTarget {
        program_handle: 101,
        program_generation: 103,
        metadata_handle: 107,
        metadata_generation: 109,
        source_id: 3,
    };
    assert!(executor.debugger_exception_location_available());
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Ok(JavaScriptPageDebuggerExceptionLocation {
            source_id: 3,
            code_unit_ordinal: 1,
            bytecode_offset: 4,
            start_byte: 2,
            end_byte: 8,
            coordinates: span.coordinates,
        })
    );

    executor.child.exception_reply = PageHostReply::DebuggerBlueTsExceptionLocation {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        program: PageHostDebuggerProgram {
            program_generation: child_program.program_generation + 1,
            ..child_program
        },
        metadata: child_metadata,
        location: page_host::PageHostDebuggerBlueTsExceptionLocation {
            safe_point: child_point,
            span,
        },
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    executor.child.exception_reply = PageHostReply::DebuggerBlueTsExceptionLocation {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        program: child_program,
        metadata: child_metadata,
        location: page_host::PageHostDebuggerBlueTsExceptionLocation {
            span: page_host::PageHostDebuggerBlueTsSafePointSpan {
                source_id: 4,
                ..span
            },
            safe_point: child_point,
        },
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    executor.child.exception_reply = exception_reply.clone();
    executor.child.point_reply = PageHostReply::DebuggerSafePointValidated {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        safe_point: PageHostDebuggerSafePoint {
            bytecode_offset: 5,
            ..child_point
        },
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    executor.child.point_reply = point_reply;
    executor.child.span_reply = PageHostReply::DebuggerBlueTsSafePointSpan {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        metadata: child_metadata,
        safe_point: child_point,
        span: page_host::PageHostDebuggerBlueTsSafePointSpan {
            end_byte: 9,
            ..span
        },
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    executor.child.exception_reply = PageHostReply::Error {
        code: PageHostErrorCode::InvalidDebuggerState,
        message: "not completed".to_string(),
    };
    assert_eq!(
        executor.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert!(executor
        .debugger_exception_location(tab_id, 2, target)
        .is_err());

    let mut denied = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    assert!(!denied.debugger_exception_location_available());
    assert_eq!(
        denied.debugger_exception_location(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

impl PageHostClient for MalformedSafePointSpanChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        self.lifecycle.synchronize_document(document)
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        self.lifecycle.close_realm(tab_id, document_generation)
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        self.lifecycle
            .debugger_realm_stats(tab_id, document_generation)
    }

    fn debugger_bluets_metadata_available(&self) -> bool {
        true
    }

    fn debugger_bluets_metadata_sources_available(&self) -> bool {
        true
    }

    fn debugger_bluets_safe_point_span_available(&self) -> bool {
        true
    }

    fn debugger_bluets_source_breakpoint_available(&self) -> bool {
        true
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }

    fn debugger_stepping_available(&self) -> bool {
        true
    }

    fn debugger_bluets_source_span_step_available(&self) -> bool {
        true
    }

    fn step_debugger_bluets_source_span(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _metadata: PageHostDebuggerMetadataHandle,
        _source_id: u32,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(self.reply.clone())
    }

    fn debugger_bluets_safe_point_span(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _metadata: PageHostDebuggerMetadataHandle,
        _safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(self.reply.clone())
    }

    fn debugger_bluets_source_breakpoint(
        &mut self,
        _tab_id: u64,
        _document_generation: u64,
        _program: PageHostDebuggerProgram,
        _metadata: PageHostDebuggerMetadataHandle,
        _source_id: u32,
        _source_byte: u32,
    ) -> io::Result<PageHostReply> {
        Ok(self.reply.clone())
    }
}

#[test]
fn core_forwards_zero_based_bluets_source_id_but_rejects_mismatched_step_echo() {
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const zero: number = 1;</script>",
        "https://example.test/zero-source.html",
    );
    let child_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let child_metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let child_safe_point = PageHostDebuggerSafePoint {
        program: child_program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    let reply = PageHostReply::DebuggerBlueTsSourceStepRequested {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        metadata: child_metadata,
        source_id: 0,
        safe_point: child_safe_point,
    };
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        MalformedSafePointSpanChild {
            lifecycle: RecordingChild::default(),
            reply: reply.clone(),
        },
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    executor.debugger_programs.insert(
        tab_id,
        BTreeMap::from([(
            child_program,
            CoreDebuggerProgram {
                program_handle: 101,
                program_generation: 103,
            },
        )]),
    );
    executor.debugger_static_metadata.insert(
        tab_id,
        BTreeMap::from([(
            child_metadata,
            CoreDebuggerStaticMetadata {
                program: child_program,
                metadata_handle: 107,
                metadata_generation: 109,
            },
        )]),
    );
    let target = JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
        program_handle: 101,
        program_generation: 103,
        metadata_handle: 107,
        metadata_generation: 109,
        source_id: 0,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    assert!(executor.debugger_source_span_stepping_available());
    assert_eq!(
        executor.step_debugger_bluets_source_span(tab_id, 1, target),
        Ok(())
    );
    executor.child.reply = PageHostReply::DebuggerBlueTsSourceStepRequested {
        tab_id: tab_id.as_u64(),
        document_generation: 1,
        metadata: child_metadata,
        source_id: 1,
        safe_point: child_safe_point,
    };
    assert_eq!(
        executor.step_debugger_bluets_source_span(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_rejects_mismatched_child_safe_point_span_envelopes() {
    let (tabs, tab_id) = loaded_tabs(
        "<script type=\"application/x-blueice-typescript\">const mapped: number = 42;</script>",
        "https://example.test/forged-span.html",
    );
    let child_program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let child_metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let child_safe_point = PageHostDebuggerSafePoint {
        program: child_program,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    let span = page_host::PageHostDebuggerBlueTsSafePointSpan {
        source_id: 3,
        start_byte: 0,
        end_byte: 5,
        coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
            start_line: 0,
            start_column_utf16: 0,
            end_line: 0,
            end_column_utf16: 5,
        },
    };
    let span_reply = |reply_tab_id, reply_generation, metadata, safe_point, span| {
        PageHostReply::DebuggerBlueTsSafePointSpan {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            metadata,
            safe_point,
            span,
        }
    };
    let reply = span_reply(tab_id.as_u64(), 1, child_metadata, child_safe_point, span);
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(MalformedSafePointSpanChild {
        lifecycle: RecordingChild::default(),
        reply: reply.clone(),
    });
    executor.synchronize_and_execute(&tabs).unwrap();
    executor.debugger_programs.insert(
        tab_id,
        BTreeMap::from([(
            child_program,
            CoreDebuggerProgram {
                program_handle: 101,
                program_generation: 103,
            },
        )]),
    );
    executor.debugger_static_metadata.insert(
        tab_id,
        BTreeMap::from([(
            child_metadata,
            CoreDebuggerStaticMetadata {
                program: child_program,
                metadata_handle: 107,
                metadata_generation: 109,
            },
        )]),
    );
    let target = JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
        program_handle: 101,
        program_generation: 103,
        metadata_handle: 107,
        metadata_generation: 109,
        source_id: 3,
        code_unit_ordinal: 0,
        bytecode_offset: 4,
    };
    assert_eq!(
        executor
            .debugger_static_metadata_safe_point_span(tab_id, 1, target)
            .unwrap(),
        JavaScriptPageDebuggerStaticMetadataSafePointSpan {
            source_id: 3,
            start_byte: 0,
            end_byte: 5,
            coordinates: span.coordinates,
        }
    );
    for malformed in [
        span_reply(
            tab_id.as_u64() + 1,
            1,
            child_metadata,
            child_safe_point,
            span,
        ),
        span_reply(tab_id.as_u64(), 2, child_metadata, child_safe_point, span),
        span_reply(
            tab_id.as_u64(),
            1,
            PageHostDebuggerMetadataHandle {
                metadata_generation: 20,
                ..child_metadata
            },
            child_safe_point,
            span,
        ),
        span_reply(
            tab_id.as_u64(),
            1,
            child_metadata,
            PageHostDebuggerSafePoint {
                bytecode_offset: 5,
                ..child_safe_point
            },
            span,
        ),
        span_reply(
            tab_id.as_u64(),
            1,
            child_metadata,
            child_safe_point,
            page_host::PageHostDebuggerBlueTsSafePointSpan {
                end_byte: span.start_byte,
                ..span
            },
        ),
        span_reply(
            tab_id.as_u64(),
            1,
            child_metadata,
            child_safe_point,
            page_host::PageHostDebuggerBlueTsSafePointSpan {
                end_byte: DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES + 1,
                ..span
            },
        ),
        span_reply(
            tab_id.as_u64(),
            1,
            child_metadata,
            child_safe_point,
            page_host::PageHostDebuggerBlueTsSafePointSpan {
                coordinates: blueice_ipc::debugger::DebuggerSourceCoordinates {
                    start_column_utf16: span.end_byte + 1,
                    ..span.coordinates
                },
                ..span
            },
        ),
    ] {
        executor.child.reply = malformed;
        assert_eq!(
            executor.debugger_static_metadata_safe_point_span(tab_id, 1, target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }
    executor.child.reply = span_reply(
        tab_id.as_u64(),
        1,
        child_metadata,
        child_safe_point,
        page_host::PageHostDebuggerBlueTsSafePointSpan {
            source_id: 4,
            ..span
        },
    );
    assert_eq!(
        executor.debugger_static_metadata_safe_point_span(tab_id, 1, target),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );

    let breakpoint_target = JavaScriptPageDebuggerStaticMetadataSourceBreakpointTarget {
        program_handle: 101,
        program_generation: 103,
        metadata_handle: 107,
        metadata_generation: 109,
        source_id: 3,
        source_byte: 2,
    };
    let breakpoint_reply =
        |reply_tab_id, reply_generation, program, metadata, source_id, source_byte, safe_point| {
            PageHostReply::DebuggerBlueTsSourceBreakpoint {
                tab_id: reply_tab_id,
                document_generation: reply_generation,
                program,
                metadata,
                source_id,
                source_byte,
                safe_point,
            }
        };
    executor.child.reply = breakpoint_reply(
        tab_id.as_u64(),
        1,
        child_program,
        child_metadata,
        3,
        2,
        Some(child_safe_point),
    );
    assert_eq!(
        executor
            .debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target)
            .unwrap(),
        Some(JavaScriptPageDebuggerSafePoint {
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        })
    );
    executor.child.reply = breakpoint_reply(
        tab_id.as_u64(),
        1,
        child_program,
        child_metadata,
        3,
        2,
        None,
    );
    assert_eq!(
        executor
            .debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target)
            .unwrap(),
        None
    );
    for malformed in [
        breakpoint_reply(8, 1, child_program, child_metadata, 3, 2, None),
        breakpoint_reply(
            tab_id.as_u64(),
            2,
            child_program,
            child_metadata,
            3,
            2,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            PageHostDebuggerProgram {
                program_generation: 14,
                ..child_program
            },
            child_metadata,
            3,
            2,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            PageHostDebuggerMetadataHandle {
                metadata_generation: 20,
                ..child_metadata
            },
            3,
            2,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            child_metadata,
            4,
            2,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            child_metadata,
            3,
            3,
            None,
        ),
        breakpoint_reply(
            tab_id.as_u64(),
            1,
            child_program,
            child_metadata,
            3,
            2,
            Some(PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_generation: 14,
                    ..child_program
                },
                ..child_safe_point
            }),
        ),
    ] {
        executor.child.reply = malformed;
        assert_eq!(
            executor.debugger_static_metadata_source_breakpoint(tab_id, 1, breakpoint_target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
    }
}

#[test]
fn core_proxies_real_child_root_safe_point_pause_and_resume_without_private_ids() {
    use crate::debugger::handle_debugger_request_with_page_javascript_executor;
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerPageRealm, DebuggerReply,
        DebuggerRequest,
    };

    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        "<script>let first = 1; first += 1; globalThis.answer = first;</script>",
        "https://example.test/root-safe-point.html",
    );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(&path, &token)
            .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let DebuggerReply::Capabilities(capabilities) =
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::DescribeCapabilities { realm },
        )
    else {
        panic!("expected OOP debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::PauseResume
            && report.state == DebuggerCapabilityState::Available
    }));
    let program = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm },
    ) {
        DebuggerReply::Programs(programs) => programs[0],
        reply => panic!("expected public OOP debugger program, got {reply:?}"),
    };
    assert!(
        program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
        "core must not disclose child-private program IDs"
    );
    let safe_point = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) {
        DebuggerReply::SafePoints(safe_points) => safe_points
            .into_iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset != 0)
            .expect("fixture has a resumable root safe point"),
        reply => panic!("expected public OOP safe points, got {reply:?}"),
    };
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ArmRootSafePointBreakpoint { safe_point },
        ),
        DebuggerReply::RootSafePointBreakpointArmed { safe_point }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Paused { safe_point },
        }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ResumeExecution { program },
        ),
        DebuggerReply::ExecutionResumed { program }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Completed,
        }
    );
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed { ordinal: 0, .. }]
    ));
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn core_proxies_real_child_nested_frame_without_exposing_private_program_ids() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        "<script>function inner() { return 4; } globalThis.answer = inner() + 1;</script>",
        "https://example.test/nested-frame.html",
    );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(&path, &token)
            .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(executor.debugger_nested_frames_available());
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    assert!(program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START);
    let target = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
        .unwrap();
    executor
        .arm_debugger_nested_safe_point_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            target.code_unit_ordinal,
            target.bytecode_offset,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
        frame,
        bytecode_offset,
    }) = executor
        .debugger_nested_execution_state(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
    else {
        panic!("the exact nested invocation must be visible to core");
    };
    assert_eq!(bytecode_offset, target.bytecode_offset);
    assert_eq!(frame.tab_id, tab_id);
    assert_eq!(frame.document_generation, 1);
    assert_eq!(frame.program_handle, program.program_handle);
    assert_eq!(frame.program_generation, program.program_generation);
    assert_eq!(frame.code_unit_ordinal, 1);
    assert_ne!(frame.frame_handle, 0);
    let stack = executor
        .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 1, 1)
        .unwrap();
    assert_eq!(stack.frames.len(), 1);
    assert_eq!(stack.frames[0].code_unit_ordinal, 1);
    assert_eq!(stack.frames[0].bytecode_offset, bytecode_offset);
    assert!(stack.stack_truncated);
    assert_eq!(
        executor.debugger_stack_snapshot(
            tab_id,
            1,
            program,
            Some(JavaScriptPageDebuggerFrame {
                frame_handle: frame.frame_handle + 1,
                ..frame
            }),
            2,
            256,
        ),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert_eq!(
        executor.debugger_stack_snapshot(tab_id, 1, program, None, 2, 256,),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert_eq!(
        executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
            frame,
            bytecode_offset,
        })
    );
    assert_eq!(
        executor.step_debugger_nested_instruction(JavaScriptPageDebuggerFrame {
            frame_handle: frame.frame_handle + 1,
            ..frame
        }),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    let mut returned = false;
    for _ in 0..96 {
        executor.step_debugger_nested_instruction(frame).unwrap();
        assert_eq!(
            executor
                .debugger_nested_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            Some(JavaScriptPageDebuggerNestedExecutionState::Stepping { frame })
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        match executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
        {
            Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
                frame: same_frame, ..
            }) => assert_eq!(same_frame, frame),
            None => {
                returned = true;
                break;
            }
            state => panic!("unexpected child frame state: {state:?}"),
        }
    }
    assert!(returned);
    assert_eq!(
        executor.step_debugger_nested_instruction(frame),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert!(matches!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 0,
            ..
        }
    ));
    let root_stack = executor
        .debugger_stack_snapshot(tab_id, 1, program, None, 2, 256)
        .unwrap();
    assert_eq!(root_stack.frames.len(), 1);
    assert_eq!(root_stack.frames[0].code_unit_ordinal, 0);
    assert!(!root_stack.stack_truncated);
    executor
        .resume_debugger_execution(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Completed
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);

    // A replacement executor starts with the same core program counter,
    // but its process-unique frame handle must not alias the predecessor.
    let (next_path, next_token, next_child) = spawn_child();
    let mut replacement =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
            &next_path,
            &next_token,
        )
        .unwrap();
    replacement.synchronize_and_execute(&tabs).unwrap();
    let next_program = replacement.debugger_programs(tab_id, 1).unwrap()[0];
    let next_target = replacement
        .debugger_safe_points(
            tab_id,
            1,
            next_program.program_handle,
            next_program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
        .unwrap();
    replacement
        .arm_debugger_nested_safe_point_breakpoint(
            tab_id,
            1,
            next_program.program_handle,
            next_program.program_generation,
            next_target.code_unit_ordinal,
            next_target.bytecode_offset,
        )
        .unwrap();
    replacement.synchronize_and_execute(&tabs).unwrap();
    let Some(JavaScriptPageDebuggerNestedExecutionState::Paused {
        frame: next_frame, ..
    }) = replacement
        .debugger_nested_execution_state(
            tab_id,
            1,
            next_program.program_handle,
            next_program.program_generation,
        )
        .unwrap()
    else {
        panic!("the replacement child must pause its own frame");
    };
    assert_eq!(next_frame.program_handle, frame.program_handle);
    assert_ne!(next_frame.frame_handle, frame.frame_handle);
    assert_eq!(
        replacement.step_debugger_nested_instruction(frame),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    replacement.close_page(tab_id);
    assert_eq!(
        replacement.step_debugger_nested_instruction(next_frame),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    drop(replacement);
    shutdown_child(&next_path, &next_token);
    next_child.join().unwrap();
    let _ = std::fs::remove_file(next_path);
}

#[test]
fn core_proxies_exact_nested_resume_to_real_bluets_child_and_revokes_handle() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript\">function inner(): number { return 4; } globalThis.answer = inner() + 1;</script>",
            "https://example.test/private-nested-resume.html",
        );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(&path, &token)
            .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let target = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
        .unwrap();
    executor
        .arm_debugger_nested_safe_point_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            target.code_unit_ordinal,
            target.bytecode_offset,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
        .debugger_nested_execution_state(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
    else {
        panic!("BlueTS child must pause under a core-owned handle");
    };
    assert_eq!(
        executor.resume_debugger_nested_execution(JavaScriptPageDebuggerFrame {
            frame_handle: frame.frame_handle + 1,
            ..frame
        }),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    executor.resume_debugger_nested_execution(frame).unwrap();
    assert_eq!(
        executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        Some(JavaScriptPageDebuggerNestedExecutionState::Resuming { frame })
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        None
    );
    assert_eq!(
        executor.resume_debugger_nested_execution(frame),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert!(matches!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Paused {
            code_unit_ordinal: 0,
            ..
        }
    ));
    executor
        .resume_debugger_execution(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor
            .debugger_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap(),
        JavaScriptPageDebuggerExecutionState::Completed
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn real_bluets_child_private_stack_is_bounded_before_public_queries() {
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerPageRealm, DebuggerReply,
        DebuggerRequest,
    };

    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
            "<script type=\"application/x-blueice-typescript\">function inner(a: number, b: number): number { let first: number = a; let second: number = b; return first + second; } globalThis.answer = inner(1, 2);</script>",
            "https://example.test/private-stack.html",
        );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(&path, &token)
            .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let DebuggerReply::Capabilities(capabilities) =
        crate::debugger::handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::DescribeCapabilities { realm },
        )
    else {
        panic!("the debugger must describe its actual capability boundary");
    };
    for capability in [DebuggerCapability::Stack, DebuggerCapability::Scopes] {
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == capability && report.state == DebuggerCapabilityState::Available
        }));
    }
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let target = executor
        .debugger_safe_points(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
        .into_iter()
        .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
        .unwrap();
    executor
        .arm_debugger_nested_safe_point_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            1,
            0,
        )
        .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
        .debugger_nested_execution_state(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
        )
        .unwrap()
    else {
        panic!("the BlueTS child must pause before its first instruction");
    };
    assert_eq!(frame.code_unit_ordinal, target.code_unit_ordinal);
    let mut full = None;
    for _ in 0..96 {
        let snapshot = executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
            .unwrap();
        if snapshot.frames[0].scope_entries.len() >= 2 {
            full = Some(snapshot);
            break;
        }
        executor.step_debugger_nested_instruction(frame).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(matches!(
            executor
                .debugger_nested_execution_state(
                    tab_id,
                    1,
                    program.program_handle,
                    program.program_generation,
                )
                .unwrap(),
            Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame: same, .. })
                if same == frame
        ));
    }
    let full = full.expect("the child must enter a scope with two active slots");
    assert_eq!(full.frames.len(), 2);
    assert_eq!(full.frames[0].code_unit_ordinal, 1);
    assert_eq!(full.frames[1].code_unit_ordinal, 0);
    assert!(!full.stack_truncated);
    assert!(!full.frames[0].scope_truncated);
    let limited = executor
        .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 1, 1)
        .unwrap();
    assert_eq!(limited.frames.len(), 1);
    assert_eq!(limited.frames[0].scope_entries.len(), 1);
    assert!(limited.stack_truncated);
    assert!(limited.frames[0].scope_truncated);
    assert_eq!(
        executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
            .unwrap(),
        full
    );
    assert_eq!(
        executor.debugger_stack_snapshot(tab_id, 1, program, Some(frame), 0, 1),
        Err(JavaScriptPageDebuggerError::ResourceLimit)
    );
    assert_eq!(
        executor.debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 257),
        Err(JavaScriptPageDebuggerError::ResourceLimit)
    );
    assert_eq!(
        executor.debugger_stack_snapshot(
            tab_id,
            1,
            program,
            Some(JavaScriptPageDebuggerFrame {
                frame_handle: frame.frame_handle + 1,
                ..frame
            }),
            2,
            256,
        ),
        Err(JavaScriptPageDebuggerError::InvalidExecutionState)
    );
    assert_eq!(
        executor.debugger_stack_snapshot(
            tab_id,
            1,
            JavaScriptPageDebuggerProgram {
                program_generation: program.program_generation + 1,
                ..program
            },
            Some(frame),
            2,
            256,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    );
    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script>globalThis.successor = true;</script>",
        Some("https://example.test/successor-stack.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn launcher_supervised_bluets_classic_and_module_values_and_static_scopes_cross_core() {
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerPageRealm, DebuggerReply,
        DebuggerRequest,
    };
    use blueice_launcher::bluejs_host::SpawnedBlueJsHost;

    let source = "let rootValue: number = 9; function inner(a: number): number { let childValue: number = a + 1; return childValue; } globalThis.answer = inner(3) + rootValue;";
    for (mime, slug) in [
        ("application/x-blueice-typescript", "classic"),
        ("application/x-blueice-typescript-module", "module"),
    ] {
        let (host, config) = SpawnedBlueJsHost::spawn_for_core().unwrap();
        let (mut tabs, tab_id) = loaded_tabs(
            &format!("<script type=\"{mime}\">{source}</script>"),
            &format!("https://example.test/{slug}-private-values.html"),
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                config.socket_path(),
                config.session_token(),
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let realm = DebuggerPageRealm {
            browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: tab_id.as_u64(),
            realm_generation: 1,
        };
        let DebuggerReply::Capabilities(capabilities) =
            crate::debugger::handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::DescribeCapabilities { realm },
            )
        else {
            panic!("{slug} debugger must describe public capabilities");
        };
        assert!(capabilities.reports.iter().any(|report| {
            report.capability == DebuggerCapability::BoundedValues
                && report.state == DebuggerCapabilityState::Planned
        }));
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        assert!(program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START);
        let nested_entry = executor
            .debugger_safe_points(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap();
        executor
            .arm_debugger_nested_safe_point_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                nested_entry.code_unit_ordinal,
                nested_entry.bytecode_offset,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
        else {
            panic!("{slug} nested BlueTS frame must pause");
        };
        let mut stack = executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
            .unwrap();
        assert_eq!(stack.frames.len(), 2);
        let mut values_ready = false;
        for _ in 0..96 {
            values_ready = [(0, 3.0_f64), (1, 9.0_f64)]
                .into_iter()
                .all(|(index, expected)| {
                    stack.frames[index]
                        .scope_entries
                        .iter()
                        .copied()
                        .any(|entry| {
                            executor.debugger_value_snapshot(
                                tab_id,
                                1,
                                JavaScriptPageDebuggerValueTarget {
                                    program,
                                    frame: Some(frame),
                                    frame_index: index as u32,
                                    safe_point: JavaScriptPageDebuggerSafePoint {
                                        code_unit_ordinal: stack.frames[index].code_unit_ordinal,
                                        bytecode_offset: stack.frames[index].bytecode_offset,
                                    },
                                    scope_entry: entry,
                                },
                            ) == Ok(JavaScriptPageDebuggerValuePreview::NumberBits(
                                expected.to_bits(),
                            ))
                        })
                });
            if values_ready {
                break;
            }
            executor.step_debugger_nested_instruction(frame).unwrap();
            executor.synchronize_and_execute(&tabs).unwrap();
            stack = executor
                .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 256)
                .unwrap();
        }
        assert!(
            values_ready,
            "{slug} nested and root values must become readable"
        );
        let target_for = |index: usize, scope_entry| JavaScriptPageDebuggerValueTarget {
            program,
            frame: Some(frame),
            frame_index: index as u32,
            safe_point: JavaScriptPageDebuggerSafePoint {
                code_unit_ordinal: stack.frames[index].code_unit_ordinal,
                bytecode_offset: stack.frames[index].bytecode_offset,
            },
            scope_entry,
        };
        for (index, expected) in [(0, 3.0_f64), (1, 9.0_f64)] {
            let reads: Vec<_> = stack.frames[index]
                .scope_entries
                .iter()
                .copied()
                .map(|entry| {
                    (
                        entry,
                        executor.debugger_value_snapshot(tab_id, 1, target_for(index, entry)),
                    )
                })
                .collect();
            assert!(
                reads.iter().any(|(_, read)| *read
                    == Ok(JavaScriptPageDebuggerValuePreview::NumberBits(
                        expected.to_bits()
                    ))),
                "{slug} frame {index} must expose its own exact active number: {reads:?}"
            );
        }
        let parent_slot = stack.frames[1]
            .scope_entries
            .iter()
            .copied()
            .find(|entry| {
                executor.debugger_value_snapshot(tab_id, 1, target_for(1, *entry))
                    == Ok(JavaScriptPageDebuggerValuePreview::NumberBits(
                        9.0_f64.to_bits(),
                    ))
            })
            .expect("nested parent root must retain the initialized BlueTS binding");
        let metadata = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()[0];
        let parent_static = JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: target_for(1, parent_slot),
        };
        assert_eq!(
            executor
                .debugger_static_scope_relation(tab_id, 1, parent_static)
                .unwrap()
                .target,
            parent_static,
            "{slug} nested parent root must cross the real child/core route"
        );
        assert!(executor
            .debugger_static_scope_relation(
                tab_id,
                1,
                JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                    metadata,
                    target: target_for(0, stack.frames[0].scope_entries[0]),
                },
            )
            .is_err());
        for denied in [
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                metadata: JavaScriptPageDebuggerStaticMetadata {
                    metadata_generation: metadata.metadata_generation + 1,
                    ..metadata
                },
                target: target_for(1, parent_slot),
            },
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                metadata,
                target: JavaScriptPageDebuggerValueTarget {
                    scope_entry: JavaScriptPageDebuggerScopeEntry {
                        slot_ordinal: parent_slot.slot_ordinal + 1_000,
                        ..parent_slot
                    },
                    ..target_for(1, parent_slot)
                },
            },
            JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
                metadata,
                target: JavaScriptPageDebuggerValueTarget {
                    safe_point: JavaScriptPageDebuggerSafePoint {
                        bytecode_offset: stack.frames[1].bytecode_offset + 1,
                        ..target_for(1, parent_slot).safe_point
                    },
                    ..target_for(1, parent_slot)
                },
            },
        ] {
            assert!(executor
                .debugger_static_scope_relation(tab_id, 1, denied)
                .is_err());
        }
        let selected = target_for(0, stack.frames[0].scope_entries[0]);
        assert_eq!(
            executor.debugger_value_snapshot(
                tab_id,
                1,
                JavaScriptPageDebuggerValueTarget {
                    safe_point: JavaScriptPageDebuggerSafePoint {
                        bytecode_offset: selected.safe_point.bytecode_offset + 1,
                        ..selected.safe_point
                    },
                    ..selected
                },
            ),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        assert_eq!(
            executor.debugger_value_snapshot(
                tab_id,
                1,
                JavaScriptPageDebuggerValueTarget {
                    frame: Some(JavaScriptPageDebuggerFrame {
                        frame_handle: frame.frame_handle + 1,
                        ..frame
                    }),
                    ..selected
                },
            ),
            Err(JavaScriptPageDebuggerError::InvalidExecutionState)
        );
        executor.resume_debugger_nested_execution(frame).unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        assert!(executor
            .debugger_static_scope_relation(tab_id, 1, parent_static)
            .is_err());
        let root = executor
            .debugger_stack_snapshot(tab_id, 1, program, None, 1, 256)
            .unwrap();
        assert_eq!(root.frames.len(), 1);
        let root_target = root.frames[0]
            .scope_entries
            .iter()
            .copied()
            .find_map(|entry| {
                let target = JavaScriptPageDebuggerValueTarget {
                    program,
                    frame: None,
                    frame_index: 0,
                    safe_point: JavaScriptPageDebuggerSafePoint {
                        code_unit_ordinal: 0,
                        bytecode_offset: root.frames[0].bytecode_offset,
                    },
                    scope_entry: entry,
                };
                (executor.debugger_value_snapshot(tab_id, 1, target)
                    == Ok(JavaScriptPageDebuggerValuePreview::NumberBits(
                        9.0_f64.to_bits(),
                    )))
                .then_some(target)
            })
            .expect("resumed root must retain its own initialized binding");
        let metadata = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()[0];
        let static_target = JavaScriptPageDebuggerStaticScopeTarget::Ordinary {
            metadata,
            target: root_target,
        };
        assert_eq!(
            executor
                .debugger_static_scope_relation(tab_id, 1, static_target)
                .unwrap()
                .target,
            static_target,
            "{slug} root static relation must cross the real child/core route"
        );
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<p>successor</p>",
            Some(format!("https://example.test/{slug}-successor.html")),
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        assert_eq!(
            executor.debugger_value_snapshot(tab_id, 1, root_target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        assert_eq!(
            executor.debugger_static_scope_relation(tab_id, 1, static_target),
            Err(JavaScriptPageDebuggerError::NoLiveRealm)
        );
        drop(executor);
        drop(host);
    }
}

#[test]
fn paused_bluets_classic_and_module_stack_frames_have_exact_original_spans() {
    let source = "/* 🚀 */ function inner(a: number): number { let value: number = a + 1; return value; } globalThis.answer = inner(3);";
    let function_start = source.find("function inner").unwrap();
    let function_end = source.find("} globalThis").unwrap() + 1;
    let call_start = source.find("globalThis.answer").unwrap();
    for (mime, slug) in [
        ("application/x-blueice-typescript", "classic"),
        ("application/x-blueice-typescript-module", "module"),
    ] {
        let (path, token, child) = spawn_child();
        let (tabs, tab_id) = loaded_tabs(
            &format!("<script type=\"{mime}\">{source}</script>"),
            &format!("https://example.test/{slug}-stack-spans.html"),
        );
        let mut executor =
            OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(
                &path, &token,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
        let metadata = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()[0];
        let source_ids = executor
            .debugger_static_metadata_sources(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                metadata.metadata_handle,
                metadata.metadata_generation,
            )
            .unwrap();
        assert!(!source_ids.is_empty());
        let target = executor
            .debugger_safe_points(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap();
        executor
            .arm_debugger_nested_safe_point_breakpoint(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
                1,
                target.bytecode_offset,
            )
            .unwrap();
        executor.synchronize_and_execute(&tabs).unwrap();
        let Some(JavaScriptPageDebuggerNestedExecutionState::Paused { frame, .. }) = executor
            .debugger_nested_execution_state(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap()
        else {
            panic!("{slug} child must pause");
        };
        let stack = executor
            .debugger_stack_snapshot(tab_id, 1, program, Some(frame), 2, 1)
            .unwrap();
        assert_eq!(stack.frames.len(), 2);
        for stack_frame in &stack.frames {
            let spans = source_ids
                .iter()
                .filter_map(|source| {
                    executor
                        .debugger_static_metadata_safe_point_span(
                            tab_id,
                            1,
                            JavaScriptPageDebuggerStaticMetadataSafePointSpanTarget {
                                program_handle: program.program_handle,
                                program_generation: program.program_generation,
                                metadata_handle: metadata.metadata_handle,
                                metadata_generation: metadata.metadata_generation,
                                source_id: source.source_id,
                                code_unit_ordinal: stack_frame.code_unit_ordinal,
                                bytecode_offset: stack_frame.bytecode_offset,
                            },
                        )
                        .ok()
                })
                .collect::<Vec<_>>();
            assert_eq!(
                spans.len(),
                1,
                "{slug} frame {stack_frame:?} must have one exact source mapping"
            );
            let span = spans[0];
            let (expected_start, expected_end) = if stack_frame.code_unit_ordinal == 1 {
                (function_start, function_end)
            } else {
                assert_eq!(stack_frame.code_unit_ordinal, 0);
                (call_start, source.len())
            };
            assert_eq!(
                (span.start_byte as usize, span.end_byte as usize),
                (expected_start, expected_end),
                "{slug} frame must use its owning original BlueTS statement"
            );
            assert_eq!(span.coordinates.start_line, 0);
            assert_eq!(
                span.coordinates.start_column_utf16,
                source[..expected_start].encode_utf16().count() as u32
            );
            assert_eq!(span.coordinates.end_line, 0);
            assert_eq!(
                span.coordinates.end_column_utf16,
                source[..expected_end].encode_utf16().count() as u32
            );
            assert!(source_ids
                .iter()
                .any(|source| source.source_id == span.source_id));
            assert!(span
                .coordinates
                .is_well_formed_for_range(span.start_byte, span.end_byte));
        }
        drop(executor);
        shutdown_child(&path, &token);
        child.join().unwrap();
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn public_core_route_steps_one_real_child_nested_frame_without_root_aliasing() {
    use crate::debugger::handle_debugger_request_with_page_javascript_executor;
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerExecutionState, DebuggerPageRealm,
        DebuggerReply, DebuggerRequest,
    };

    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        "<script>function inner() { return 4; } globalThis.answer = inner() + 1;</script>",
        "https://example.test/public-nested.html",
    );
    let mut executor =
        OutOfProcessJavaScriptPageExecutor::connect_with_debugger_execution_control(&path, &token)
            .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let DebuggerReply::Capabilities(capabilities) =
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::DescribeCapabilities { realm },
        )
    else {
        panic!("expected live debugger capabilities");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == DebuggerCapability::NestedFrames
            && report.state == DebuggerCapabilityState::Available
    }));
    let program = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm },
    ) {
        DebuggerReply::Programs(programs) => programs[0],
        reply => panic!("expected one program: {reply:?}"),
    };
    let target = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) {
        DebuggerReply::SafePoints(points) => points
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .unwrap(),
        reply => panic!("expected nested safe points: {reply:?}"),
    };
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ArmNestedSafePointBreakpoint { safe_point: target },
        ),
        DebuggerReply::NestedSafePointBreakpointArmed { safe_point: target }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let frame = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::GetExecutionState { program },
    ) {
        DebuggerReply::ExecutionState {
            state: DebuggerExecutionState::NestedPaused { frame, safe_point },
            ..
        } => {
            assert_eq!(safe_point, target);
            frame
        }
        reply => panic!("expected a public active frame: {reply:?}"),
    };
    assert_eq!(frame.program, program);
    assert_ne!(frame.frame_handle, 0);
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::StepRootInstruction { program },
        ),
        DebuggerReply::Error { .. }
    ));
    let mut wrong = frame;
    wrong.frame_handle += 1;
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::StepNestedInstruction { frame: wrong },
        ),
        DebuggerReply::Error { .. }
    ));
    let mut returned = false;
    for _ in 0..96 {
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::StepNestedInstruction { frame },
            ),
            DebuggerReply::NestedStepRequested { frame }
        );
        assert_eq!(
            handle_debugger_request_with_page_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: DebuggerExecutionState::NestedStepping { frame },
            }
        );
        executor.synchronize_and_execute(&tabs).unwrap();
        match handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ) {
            DebuggerReply::ExecutionState {
                state:
                    DebuggerExecutionState::NestedPaused {
                        frame: same_frame, ..
                    },
                ..
            } => assert_eq!(same_frame, frame),
            DebuggerReply::ExecutionState {
                state: DebuggerExecutionState::Paused { safe_point },
                ..
            } => {
                assert_eq!(safe_point.code_unit_ordinal, 0);
                returned = true;
                break;
            }
            reply => panic!("unexpected nested successor: {reply:?}"),
        }
    }
    assert!(returned);
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::StepNestedInstruction { frame },
        ),
        DebuggerReply::Error { .. }
    ));
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ResumeExecution { program },
        ),
        DebuggerReply::ExecutionResumed { program }
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState { program },
        ),
        DebuggerReply::ExecutionState {
            program,
            state: DebuggerExecutionState::Completed,
        }
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

/// A hostile authenticated child still cannot acknowledge a different
/// realm and make core treat the requested document as eligible for any
/// debugger lifecycle record.
struct MismatchedSynchronizationChild;

impl PageHostClient for MismatchedSynchronizationChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id.saturating_add(1),
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn debugger_execution_control_available(&self) -> bool {
        true
    }
}

#[test]
fn mismatched_child_synchronization_cannot_seed_oop_debugger_lifecycle() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let childLifecycleSecret = 1;</script>",
        "https://example.test/hostile-child.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new_with_debugger_execution_control(
        MismatchedSynchronizationChild,
    );
    executor.synchronize_and_execute(&tabs).unwrap();

    assert!(!executor.debugger_has_live_realm(tab_id, 1));
    assert!(executor.debugger_execution_deferrals.is_empty());
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![JavaScriptPageExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: BlueJsPageScriptKind::Classic,
            category: "out-of-process JavaScript host is unavailable",
        }]
    );
}

struct MismatchedDebuggerChild;

impl PageHostClient for MismatchedDebuggerChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmStats(page_host::PageHostRealmStats {
            tab_id,
            document_generation,
            program_count: 1,
            bytecode_bytes: 0,
            heap_bytes: 0,
        }))
    }

    fn debugger_programs(
        &mut self,
        _tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerPrograms {
            // A peer must never be able to retarget core's tab merely by
            // returning a plausible private record under another tuple.
            tab_id: 99,
            document_generation,
            programs: vec![PageHostDebuggerProgram {
                program_handle: 1,
                program_generation: 1,
            }],
        })
    }
}

#[test]
fn core_rejects_a_child_debugger_reply_with_a_mismatched_realm_tuple() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let noChildSourceLeak = true;</script>",
        "https://example.test/app.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(MismatchedDebuggerChild);
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(executor.debugger_has_live_realm(tab_id, 1));
    let realm = blueice_ipc::debugger::DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: tab_id.as_u64(),
        realm_generation: 1,
    };
    let capabilities = crate::debugger::handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        blueice_ipc::debugger::DebuggerRequest::DescribeCapabilities { realm },
    );
    let blueice_ipc::debugger::DebuggerReply::Capabilities(capabilities) = capabilities else {
        panic!("expected child debugger capability report");
    };
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::ProgramLocations
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::BreakpointConfiguration
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned
    }));
    assert_eq!(
        executor.debugger_programs(tab_id, 1),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

/// A hostile private peer can know its own protocol shape, so core must
/// reject malformed breakpoint acknowledgements rather than treating an
/// authenticated socket as a source of public debugger identities.
struct MalformedBreakpointChild;

impl PageHostClient for MalformedBreakpointChild {
    fn synchronize_document(&mut self, document: PageHostDocument) -> io::Result<PageHostReply> {
        Ok(PageHostReply::Synchronized {
            tab_id: document.tab_id,
            document_generation: document.document_generation,
            already_current: false,
            reports: Vec::new(),
        })
    }

    fn close_realm(&mut self, tab_id: u64, document_generation: u64) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmClosed {
            tab_id,
            document_generation,
        })
    }

    fn debugger_realm_stats(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::RealmStats(page_host::PageHostRealmStats {
            tab_id,
            document_generation,
            program_count: 1,
            bytecode_bytes: 0,
            heap_bytes: 0,
        }))
    }

    fn debugger_programs(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerPrograms {
            tab_id,
            document_generation,
            programs: vec![PageHostDebuggerProgram {
                program_handle: 1,
                program_generation: 1,
            }],
        })
    }

    fn validate_debugger_safe_point(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerSafePointValidated {
            tab_id,
            document_generation,
            safe_point,
        })
    }

    fn set_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerBreakpointSet {
            // The reply is otherwise plausible, but a core must bind it
            // to the original tab and never silently retarget a request.
            tab_id: tab_id + 1,
            document_generation,
            safe_point,
        })
    }

    fn debugger_breakpoints(
        &mut self,
        tab_id: u64,
        document_generation: u64,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerBreakpoints {
            tab_id,
            // A generation mismatch must not reveal or remint a record.
            document_generation: document_generation + 1,
            safe_points: Vec::new(),
        })
    }

    fn clear_debugger_breakpoint(
        &mut self,
        tab_id: u64,
        document_generation: u64,
        safe_point: PageHostDebuggerSafePoint,
    ) -> io::Result<PageHostReply> {
        Ok(PageHostReply::DebuggerBreakpointCleared {
            tab_id,
            document_generation,
            // A response must echo the exact child-private safe point,
            // not merely one with a valid private program ID.
            safe_point: PageHostDebuggerSafePoint {
                bytecode_offset: safe_point.bytecode_offset.saturating_add(1),
                ..safe_point
            },
            was_present: true,
        })
    }
}

#[test]
fn core_fails_closed_on_malformed_child_breakpoint_replies() {
    let (tabs, tab_id) = loaded_tabs(
        "<script>let childBreakpointSecret = 1;</script>",
        "https://example.test/app.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(MalformedBreakpointChild);
    executor.synchronize_and_execute(&tabs).unwrap();
    let program = executor.debugger_programs(tab_id, 1).unwrap()[0];
    let result = executor.set_debugger_breakpoint(
        tab_id,
        1,
        program.program_handle,
        program.program_generation,
        0,
        0,
    );
    assert_eq!(result, Err(JavaScriptPageDebuggerError::NoLiveRealm));
    assert_eq!(
        executor.debugger_breakpoints(tab_id, 1),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
    assert_eq!(
        executor.clear_debugger_breakpoint(
            tab_id,
            1,
            program.program_handle,
            program.program_generation,
            0,
            0,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    );
}

#[test]
fn core_validates_and_installs_source_free_document_snapshots_in_the_real_child() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
        concat!(
            "<p>core snapshot marker</p>",
            "<script>",
            "if (blueiceDocumentOrigin() !== 'https://example.test') throw 'origin';",
            "if (blueiceDocumentText() === '') throw 'text';",
            "if (typeof document !== 'undefined' || typeof fetch !== 'undefined') throw 'ambient';",
            "</script>"
        ),
        "https://example.test/app/index.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![JavaScriptPageExecutionReport::Executed {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: BlueJsPageScriptKind::Classic,
        }]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn over_budget_core_snapshot_is_never_sent_to_the_child() {
    let oversized = "x".repeat(blueice_ipc::page_host::PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES + 1);
    let html = format!("<p>{oversized}</p><script>blueiceDocumentText();</script>");
    let (tabs, tab_id) = loaded_tabs(&html, "https://example.test/app/index.html");
    let mut executor = OutOfProcessJavaScriptPageExecutor::new(RecordingChild::default());
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![JavaScriptPageExecutionReport::Rejected {
            tab_id: tab_id.as_u64(),
            document_generation: 1,
            ordinal: 0,
            kind: BlueJsPageScriptKind::Classic,
            category: "host binding contract rejected the page script",
        }]
    );
    let child = executor.into_child();
    assert!(child.documents.is_empty());
    assert!(child.closes.is_empty());
}

#[test]
fn core_routes_interleaved_bluets_and_javascript_to_one_child_realm() {
    let (path, token, child) = spawn_child();
    let (tabs, tab_id) = loaded_tabs(
            concat!(
                "<script>globalThis.beforeBlueTs = true;</script>",
                "<script type=\"application/x-blueice-typescript\">const sharedAnswer: number = 42;</script>",
                "<script>if (!globalThis.beforeBlueTs || sharedAnswer !== 42) throw 'shared realm failed';</script>",
                "<script type=\"application/x-blueice-typescript\">blueiceDocumentText();</script>",
                "<script type=\"application/x-blueice-typescript\" src=\"untrusted.ts\"></script>"
            ),
            "https://example.test/app/index.html",
        );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        executor.drain_reports_for_tab(tab_id),
        vec![
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 0,
                kind: BlueJsPageScriptKind::Classic,
            },
            JavaScriptPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 2,
                kind: BlueJsPageScriptKind::Classic,
            },
        ]
    );
    assert_eq!(
        executor.drain_blue_ts_reports_for_tab(tab_id),
        vec![
            BlueTsPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 1,
                kind: DirectPageScriptKind::Classic,
            },
            BlueTsPageExecutionReport::Executed {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 3,
                kind: DirectPageScriptKind::Classic,
            },
            BlueTsPageExecutionReport::Rejected {
                tab_id: tab_id.as_u64(),
                document_generation: 1,
                ordinal: 4,
                kind: DirectPageScriptKind::Classic,
                category: "external BlueTS declarations require an authorized loader",
            },
        ]
    );
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn navigation_replaces_the_child_document_and_close_releases_its_realm() {
    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
        "<script>globalThis.firstSnapshot = blueiceDocumentText();</script>",
        "https://example.test/first",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            document_generation: 1,
            ..
        }]
    ));
    tabs.get_mut(tab_id).unwrap().load_html_str(
        concat!(
            "<script>",
            "if (typeof globalThis.firstSnapshot !== 'undefined') throw 'stale realm';",
            "if (blueiceDocumentOrigin() !== 'https://second.example.test') throw 'origin';",
            "globalThis.second = true;",
            "</script>"
        ),
        Some("https://second.example.test/second".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.drain_reports_for_tab(tab_id).as_slice(),
        [JavaScriptPageExecutionReport::Executed {
            document_generation: 2,
            ..
        }]
    ));
    assert!(tabs.close_tab(tab_id));
    executor.synchronize_and_execute(&tabs).unwrap();
    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}
