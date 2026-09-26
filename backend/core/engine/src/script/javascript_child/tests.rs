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
mod authorization;
mod lifecycle;
mod linked_pause;
mod metadata;
mod nested_pause;
mod private_scope;
mod resource_accounting;
mod scope_values;
mod span_contracts;
mod stack_scopes;
mod transport;

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
