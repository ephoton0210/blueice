// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared helpers for `blueice-net`'s integration tests: a hand-rolled,
//! misbehavior-capable HTTP/1.1 server, a fake gatekeeper on a real Unix
//! socket, and a temp directory.
//!
//! The server is what every engine test's credibility rests on, so it is
//! deliberately small and readable, and it has tests of its own
//! (`tests/test_server.rs`). It answers `GET` only, one request per
//! connection (`Connection: close`), and can be told to: ignore `Range`,
//! change its `ETag` mid-transfer, answer `5xx` a few times first, cut a
//! body short, stall, throttle (per range, so one segment can be made
//! slow), redirect, or send no `Content-Length`. It logs every request
//! and tracks how many bodies are streaming at once.

#![allow(dead_code)]

use blueice_ipc::gatekeeper::{read_gatekeeper_request, write_gatekeeper_reply, GatekeeperReply, GatekeeperRequest};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// A directory under the system temp dir, removed on drop. Kept short
/// (`/tmp/bn-<pid>-<n>`) so a Unix socket path inside it stays under the
/// platform limit.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("bn-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Default for TempDir {
    fn default() -> Self {
        TempDir::new()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Deterministic test content in which every position differs from its
/// neighbours' pattern, so a misplaced or duplicated range shows up as a
/// content mismatch instead of passing by luck.
pub fn body(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31) ^ (i >> 8) ^ (i >> 16)) as u8).collect()
}

/// How the server answers one path. Counters (`fail_statuses`,
/// `cut_times`, `stall_times`) are consumed as requests arrive.
#[derive(Clone)]
pub struct Resource {
    pub body: Arc<Vec<u8>>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    /// Advertise `Accept-Ranges` and answer `Range` with `206`.
    pub honor_ranges: bool,
    /// `false` sends no `Content-Length` (the body ends when the
    /// connection closes), like a chunked/streamed response.
    pub send_content_length: bool,
    /// Answer `302` to this path (relative to this server) instead of serving.
    pub redirect_to: Option<String>,
    /// Statuses to answer with, one per request, before serving normally.
    pub fail_statuses: Vec<u16>,
    /// Replace the `Content-Range` header of a `206` answer.
    pub content_range_override: Option<String>,
    /// Replace the `ETag` / `Last-Modified` header of a `206` answer only
    /// (a CDN serving ranges from a different version of the file).
    pub range_etag_override: Option<String>,
    pub range_last_modified_override: Option<String>,
    /// For the next `cut_times` bodies longer than this, send only this
    /// many bytes and close the connection.
    pub cut_body_after: Option<usize>,
    pub cut_times: usize,
    /// For the next `stall_times` bodies longer than this, send this many
    /// bytes and then go silent for `stall_hold` without closing.
    pub stall_after: Option<usize>,
    pub stall_times: usize,
    pub stall_hold: Duration,
    /// Body write granularity and the pause after each write.
    pub chunk: usize,
    pub delay_per_chunk: Duration,
    /// An extra per-chunk pause chosen by the request's range start
    /// (`None` for a request without a `Range`) -- how a test makes one
    /// segment slow so the others finish first and must split it.
    pub slow: Option<Arc<dyn Fn(Option<u64>) -> Duration + Send + Sync>>,
}

impl Resource {
    /// A well-behaved resource: strong ETag, `Last-Modified`, ranges honored.
    pub fn new(body: Vec<u8>) -> Self {
        Resource {
            body: Arc::new(body),
            etag: Some("\"v1\"".to_string()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
            content_type: Some("application/octet-stream".to_string()),
            content_disposition: None,
            honor_ranges: true,
            send_content_length: true,
            redirect_to: None,
            fail_statuses: Vec::new(),
            content_range_override: None,
            range_etag_override: None,
            range_last_modified_override: None,
            cut_body_after: None,
            cut_times: 0,
            stall_after: None,
            stall_times: 0,
            stall_hold: Duration::from_secs(5),
            chunk: 16 * 1024,
            delay_per_chunk: Duration::ZERO,
            slow: None,
        }
    }
}

/// One request the server saw, and how it answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLog {
    pub path: String,
    pub range: Option<String>,
    pub if_range: Option<String>,
    pub accept_encoding: Option<String>,
    pub status: u16,
}

#[derive(Default)]
struct Shared {
    resources: Mutex<HashMap<String, Resource>>,
    log: Mutex<Vec<RequestLog>>,
    active: AtomicUsize,
    peak: AtomicUsize,
    stop: AtomicBool,
}

pub struct TestServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

impl TestServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let shared = Arc::new(Shared::default());
        let for_accept = shared.clone();
        let accept = thread::spawn(move || {
            for connection in listener.incoming() {
                if for_accept.stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = connection else { continue };
                let for_handler = for_accept.clone();
                thread::spawn(move || handle(stream, &for_handler, addr));
            }
        });
        TestServer { addr, shared, accept: Some(accept) }
    }

    pub fn serve(&self, path: &str, resource: Resource) {
        self.shared.resources.lock().unwrap().insert(path.to_string(), resource);
    }

    /// Changes a served resource while transfers are running against it.
    pub fn update(&self, path: &str, change: impl FnOnce(&mut Resource)) {
        change(self.shared.resources.lock().unwrap().get_mut(path).expect("no such resource"));
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn requests(&self) -> Vec<RequestLog> {
        self.shared.log.lock().unwrap().clone()
    }

    /// The most response bodies ever streaming at the same moment.
    pub fn peak_concurrency(&self) -> usize {
        self.shared.peak.load(Ordering::SeqCst)
    }

    /// Bodies streaming right now.
    pub fn active(&self) -> usize {
        self.shared.active.load(Ordering::SeqCst)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr); // wake the accept loop
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

struct ActiveGuard<'a>(&'a Shared);

impl<'a> ActiveGuard<'a> {
    fn new(shared: &'a Shared) -> Self {
        let now = shared.active.fetch_add(1, Ordering::SeqCst) + 1;
        shared.peak.fetch_max(now, Ordering::SeqCst);
        ActiveGuard(shared)
    }
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return Some(String::from_utf8_lossy(&buf).into_owned());
        }
        if buf.len() > 16 * 1024 {
            return None;
        }
    }
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        206 => "Partial Content",
        302 => "Found",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

fn write_head(stream: &mut TcpStream, status: u16, headers: &[(&str, String)]) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {}\r\n", status_text(status));
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())
}

enum RangeRequest {
    None,
    Satisfiable(u64, u64),
    Unsatisfiable,
}

/// `bytes=a-b` and `bytes=a-` only; suffix ranges are ignored (treated
/// as no range), which no engine request uses.
fn parse_range(value: &str, len: u64) -> RangeRequest {
    let Some(spec) = value.trim().strip_prefix("bytes=") else { return RangeRequest::None };
    let Some((start, end)) = spec.split_once('-') else { return RangeRequest::None };
    let Ok(start) = start.trim().parse::<u64>() else { return RangeRequest::None };
    let end = if end.trim().is_empty() { len.saturating_sub(1) } else { end.trim().parse::<u64>().unwrap_or(u64::MAX).min(len.saturating_sub(1)) };
    if start >= len {
        RangeRequest::Unsatisfiable
    } else {
        RangeRequest::Satisfiable(start, end.max(start))
    }
}

fn handle(mut stream: TcpStream, shared: &Shared, addr: SocketAddr) {
    let Some(head) = read_head(&mut stream) else { return };
    let mut lines = head.lines();
    let mut request_line = lines.next().unwrap_or("").split_whitespace();
    let _method = request_line.next();
    let path = request_line.next().unwrap_or("/").split('?').next().unwrap_or("/").to_string();
    let headers: HashMap<String, String> =
        lines.filter_map(|line| line.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let range_header = headers.get("range").cloned();
    let if_range = headers.get("if-range").cloned();
    let log = |status: u16| {
        shared.log.lock().unwrap().push(RequestLog {
            path: path.clone(),
            range: range_header.clone(),
            if_range: if_range.clone(),
            accept_encoding: headers.get("accept-encoding").cloned(),
            status,
        });
    };

    // Decide what kind of answer this is, consuming a failure/redirect if any.
    let resource = {
        let mut resources = shared.resources.lock().unwrap();
        let Some(resource) = resources.get_mut(&path) else {
            drop(resources);
            log(404);
            let _ = write_head(&mut stream, 404, &[("Content-Length", "0".to_string())]);
            return;
        };
        if !resource.fail_statuses.is_empty() {
            let status = resource.fail_statuses.remove(0);
            drop(resources);
            log(status);
            let _ = write_head(&mut stream, status, &[("Content-Length", "0".to_string())]);
            return;
        }
        if let Some(target) = resource.redirect_to.clone() {
            drop(resources);
            log(302);
            let _ = write_head(&mut stream, 302, &[("Location", format!("http://{addr}{target}")), ("Content-Length", "0".to_string())]);
            return;
        }
        resource.clone()
    };

    let total = resource.body.len() as u64;
    let if_range_matches = match &if_range {
        None => true,
        Some(v) => resource.etag.as_deref() == Some(v.as_str()) || resource.last_modified.as_deref() == Some(v.as_str()),
    };
    let range = match (&range_header, resource.honor_ranges && if_range_matches) {
        (Some(value), true) => parse_range(value, total),
        _ => RangeRequest::None,
    };

    let mut response_headers: Vec<(&str, String)> = Vec::new();
    if resource.honor_ranges {
        response_headers.push(("Accept-Ranges", "bytes".to_string()));
    }
    if let Some(etag) = &resource.etag {
        response_headers.push(("ETag", etag.clone()));
    }
    if let Some(lm) = &resource.last_modified {
        response_headers.push(("Last-Modified", lm.clone()));
    }
    if let Some(ct) = &resource.content_type {
        response_headers.push(("Content-Type", ct.clone()));
    }
    if let Some(cd) = &resource.content_disposition {
        response_headers.push(("Content-Disposition", cd.clone()));
    }

    let (status, slice_start, slice_end) = match range {
        RangeRequest::Unsatisfiable => {
            response_headers.push(("Content-Range", format!("bytes */{total}")));
            response_headers.push(("Content-Length", "0".to_string()));
            log(416);
            let _ = write_head(&mut stream, 416, &response_headers);
            return;
        }
        RangeRequest::Satisfiable(a, b) => {
            let shown = resource.content_range_override.clone().unwrap_or(format!("bytes {a}-{b}/{total}"));
            response_headers.push(("Content-Range", shown));
            response_headers.push(("Content-Length", (b - a + 1).to_string()));
            for header in response_headers.iter_mut() {
                match (header.0, &resource.range_etag_override, &resource.range_last_modified_override) {
                    ("ETag", Some(etag), _) => header.1 = etag.clone(),
                    ("Last-Modified", _, Some(lm)) => header.1 = lm.clone(),
                    _ => {}
                }
            }
            (206, a as usize, b as usize + 1)
        }
        RangeRequest::None => {
            if resource.send_content_length {
                response_headers.push(("Content-Length", total.to_string()));
            }
            (200, 0, total as usize)
        }
    };
    let slice = &resource.body[slice_start..slice_end];
    let range_start = if status == 206 { Some(slice_start as u64) } else { None };

    // Consume a cut/stall only when it actually applies to this body.
    let (cut, stall) = {
        let mut resources = shared.resources.lock().unwrap();
        let live = resources.get_mut(&path).expect("resource vanished");
        let mut cut = None;
        let mut stall = None;
        if let Some(n) = live.cut_body_after.filter(|&n| slice.len() > n && live.cut_times > 0) {
            live.cut_times -= 1;
            cut = Some(n);
        } else if let Some(n) = live.stall_after.filter(|&n| slice.len() > n && live.stall_times > 0) {
            live.stall_times -= 1;
            stall = Some(n);
        }
        (cut, stall)
    };

    log(status);
    if write_head(&mut stream, status, &response_headers).is_err() {
        return;
    }

    let _active = ActiveGuard::new(shared);
    let mut sent = 0;
    while sent < slice.len() {
        if let Some(n) = stall.filter(|&n| sent >= n) {
            let _ = n;
            thread::sleep(resource.stall_hold);
            return;
        }
        if cut.is_some_and(|n| sent >= n) {
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
        let mut end = (sent + resource.chunk.max(1)).min(slice.len());
        if let Some(n) = cut.or(stall) {
            end = end.min(n);
        }
        if stream.write_all(&slice[sent..end]).is_err() {
            return;
        }
        sent = end;
        let extra = resource.slow.as_ref().map(|f| f(range_start)).unwrap_or(Duration::ZERO);
        let pause = resource.delay_per_chunk + extra;
        if !pause.is_zero() {
            thread::sleep(pause);
        }
    }
    let _ = stream.flush();
}

/// How a fake gatekeeper answers one request.
pub enum GateReply {
    Clear,
    Reject { reason: String, category: String },
    /// Accept the connection and never answer (for this long).
    Hang(Duration),
    /// Take this long to answer, then clear -- a slow but working gatekeeper.
    SlowClear(Duration),
    /// Accept the connection and hang up without answering.
    Close,
    /// Answer with bytes that are not a valid frame.
    Garbage,
}

/// A gatekeeper on a real Unix socket, recording every request it gets.
pub struct FakeGatekeeper {
    pub socket: PathBuf,
    requests: Arc<Mutex<Vec<GatekeeperRequest>>>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    _dir: Option<TempDir>,
}

impl FakeGatekeeper {
    pub fn start(policy: impl Fn(&GatekeeperRequest) -> GateReply + Send + Sync + 'static) -> Self {
        let dir = TempDir::new();
        let socket = dir.join("g.sock");
        FakeGatekeeper::start_owning(socket, Some(dir), policy)
    }

    /// Like [`Self::start`], but listening at `socket` -- for a process that
    /// looks for its gatekeeper at a well-known path rather than being told.
    pub fn start_at(socket: PathBuf, policy: impl Fn(&GatekeeperRequest) -> GateReply + Send + Sync + 'static) -> Self {
        let _ = std::fs::remove_file(&socket);
        FakeGatekeeper::start_owning(socket, None, policy)
    }

    fn start_owning(socket: PathBuf, dir: Option<TempDir>, policy: impl Fn(&GatekeeperRequest) -> GateReply + Send + Sync + 'static) -> Self {
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let policy = Arc::new(policy);
        let (for_accept, requests_for_accept, stop_for_accept) = (policy, requests.clone(), stop.clone());
        let accept = thread::spawn(move || {
            while !stop_for_accept.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let (policy, requests) = (for_accept.clone(), requests_for_accept.clone());
                        thread::spawn(move || {
                            let _ = stream.set_nonblocking(false);
                            let Ok(request) = read_gatekeeper_request(&mut stream) else { return };
                            let reply = policy(&request);
                            requests.lock().unwrap().push(request);
                            match reply {
                                GateReply::Clear => {
                                    let _ = write_gatekeeper_reply(&mut stream, &GatekeeperReply::Cleared);
                                }
                                GateReply::Reject { reason, category } => {
                                    let _ = write_gatekeeper_reply(&mut stream, &GatekeeperReply::Rejected { reason, category });
                                }
                                GateReply::Hang(how_long) => thread::sleep(how_long),
                                GateReply::SlowClear(how_long) => {
                                    thread::sleep(how_long);
                                    let _ = write_gatekeeper_reply(&mut stream, &GatekeeperReply::Cleared);
                                }
                                GateReply::Close => {}
                                GateReply::Garbage => {
                                    let bad = b"nope";
                                    let _ = stream.write_all(&(bad.len() as u32).to_le_bytes());
                                    let _ = stream.write_all(bad);
                                }
                            }
                        });
                    }
                    Err(_) => thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        FakeGatekeeper { socket, requests, stop, accept: Some(accept), _dir: dir }
    }

    pub fn clear_all() -> Self {
        FakeGatekeeper::start(|_| GateReply::Clear)
    }

    pub fn requests(&self) -> Vec<GatekeeperRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeGatekeeper {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}
