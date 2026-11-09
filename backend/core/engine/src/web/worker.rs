// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A document's scripts somewhere of their own: another thread or another process.
//!
//! The page talks to its scripts in *turns*: it says what to run and later gets
//! what the scripts did. While a turn runs the page keeps painting and answering
//! input from the last document it got; the session is not held for as long as a
//! script runs. Only one turn is in flight at a time.
//!
//! * A **thread** worker owns the realm and is handed a copy of the document,
//!   styles and layout boxes for each turn, and hands the document back.
//! * A **process** worker owns the live document in a child process. The page
//!   sends turns and gets back what they did; it fetches the document when it
//!   needs it, and answers the child's questions about layout (computed style,
//!   boxes, pictures) while a turn runs. A script that crashes, hangs or eats
//!   memory costs the page its scripts, not the page.

use super::dom_bridge::{DomState, NavigationRequest};
use super::turn::{run_turn, Turn};
use super::wire::{self, Answer, AnswerWire, ChildDone, FromChild, Query, ToChild};
use super::WebHost;
use blueice_css::ComputedStyle;
use blueice_dom::{Document, NodeId};
use std::collections::HashMap;
use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

type Rects = HashMap<NodeId, (f64, f64, f64, f64)>;
type Pictures = HashMap<NodeId, Arc<blueice_image::RasterImage>>;

/// What the page lends the scripts for one turn.
pub(crate) struct Handoff {
    pub doc: Document,
    pub styles: HashMap<NodeId, ComputedStyle>,
    pub rects: Rects,
    /// Whether `rects` is new to the worker (the page has laid out since it last had
    /// them). A thread worker is always sent them.
    pub rects_changed: bool,
    pub images: Pictures,
    pub viewport: (f64, f64),
    pub url: Option<String>,
}

/// What a turn leaves behind for the page to take over.
pub(crate) struct Returned {
    /// The document after the turn. A process worker leaves this unset: the
    /// page fetches it when it needs it.
    pub doc: Option<Document>,
    /// The page's styles back (a thread worker was lent them).
    pub styles: Option<HashMap<NodeId, ComputedStyle>>,
    pub dirty: bool,
    pub navigations: Vec<NavigationRequest>,
    pub script_requests: Vec<(NodeId, String)>,
    pub url: String,
    pub net_jobs: Vec<super::net::NetJob>,
    pub canvases: Vec<(NodeId, Arc<blueice_image::RasterImage>)>,
    pub loaded_images: Vec<(String, Arc<crate::images::LoadedImage>)>,
}

/// A finished turn.
pub(crate) struct Done {
    pub returned: Returned,
    /// When the next timer is due, on the realm's clock, and what that clock
    /// read when this was taken (at `sampled`).
    pub next_timer_due: Option<u64>,
    pub clock_ms: u64,
    pub sampled: Instant,
    /// Console lines the turn added.
    pub console: Vec<String>,
}

/// What a process worker asks the page while a turn runs.
pub(crate) trait PageQueries {
    fn computed(&self, node: NodeId, property: &str) -> Option<String>;
    fn image(&self, node: NodeId) -> Option<Arc<blueice_image::RasterImage>>;
}

/// The longest a turn may run before the page stops waiting for its process
/// and ends it. The realm's own limits (a callback's wall-clock limit, the
/// startup budget) come well before this; it is for a process that has stopped
/// answering at all.
const HARD_TURN_LIMIT: Duration = Duration::from_secs(90);

/// How many console lines the page's handle keeps.
const MAX_KEPT_CONSOLE_LINES: usize = 500;

struct Job {
    handoff: Handoff,
    turn: Turn,
}

enum Event {
    Message(FromChild),
    Closed,
}

/// How a process's turn stands.
enum Poll {
    Done(ChildDone),
    Pending,
    Failed,
}

struct ProcessLink {
    child: Child,
    stream: UnixStream,
    events: Receiver<Event>,
    /// The revision of the page's document that the process's copy matches.
    synced_revision: u64,
    turn_started: Option<Instant>,
    failed: bool,
}

enum Backend {
    Thread {
        jobs: Sender<Job>,
        done: Receiver<Done>,
    },
    Process(Box<ProcessLink>),
}

/// The page's handle on its scripts.
pub(crate) struct ScriptWorker {
    backend: Backend,
    busy: bool,
    next_timer_due: Option<u64>,
    clock_ms: u64,
    sampled: Instant,
    console: Vec<String>,
}

impl Drop for ProcessLink {
    fn drop(&mut self) {
        // Whatever the process is doing, the page is done with it.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl ScriptWorker {
    fn with(backend: Backend) -> Self {
        ScriptWorker {
            backend,
            busy: false,
            next_timer_due: None,
            clock_ms: 0,
            sampled: Instant::now(),
            console: Vec::new(),
        }
    }

    /// Starts a thread and builds the realm in it for `doc`. Fails with the
    /// reason when the realm cannot be built.
    pub fn spawn(doc: Document, url: &str, viewport: (f64, f64)) -> Result<Self, String> {
        let (jobs, job_rx) = channel::<Job>();
        let (done_tx, done) = channel::<Done>();
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();
        let url = url.to_string();
        std::thread::Builder::new()
            .name("page-scripts".into())
            // Page scripts nest deeply (React's render is hundreds of frames); the
            // core's main thread has 8 MiB and a script needs the same.
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                let state = DomState::new(doc, &url, viewport);
                let mut host = match WebHost::new(state) {
                    Ok(host) => {
                        let _ = ready_tx.send(Ok(()));
                        host
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                let mut console_sent = 0;
                for job in job_rx {
                    let Job { handoff, turn } = job;
                    host.with_state(|s| {
                        s.doc = handoff.doc;
                        s.styles = handoff.styles;
                        s.rects = handoff.rects;
                        s.images = handoff.images;
                        s.viewport = handoff.viewport;
                        if let Some(url) = handoff.url {
                            s.url = url;
                        }
                        s.dirty = false;
                    });
                    run_turn(&mut host, turn);
                    let returned = host.with_state(|s| Returned {
                        doc: Some(std::mem::replace(&mut s.doc, Document::new())),
                        styles: Some(std::mem::take(&mut s.styles)),
                        dirty: std::mem::take(&mut s.dirty),
                        navigations: std::mem::take(&mut s.navigations),
                        script_requests: std::mem::take(&mut s.script_requests),
                        url: s.url.clone(),
                        net_jobs: std::mem::take(&mut s.net_jobs),
                        canvases: s.take_canvas_pictures(),
                        loaded_images: std::mem::take(&mut s.loaded_images),
                    });
                    let (console, total) = host.console_since(console_sent);
                    console_sent = total;
                    let done = Done {
                        returned,
                        next_timer_due: host.next_timer_due(),
                        clock_ms: host.now_ms(),
                        sampled: Instant::now(),
                        console,
                    };
                    if done_tx.send(done).is_err() {
                        return;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        ready_rx
            .recv()
            .map_err(|_| "the page-scripts thread ended before it was ready".to_string())??;
        Ok(Self::with(Backend::Thread { jobs, done }))
    }

    /// Starts `exe --page-scripts-child` as a process and builds the realm in it
    /// for `doc`. Fails with the reason when the process cannot be started or
    /// cannot build the realm.
    pub fn spawn_process(
        exe: &Path,
        doc: &Document,
        url: &str,
        viewport: (f64, f64),
    ) -> Result<Self, String> {
        let dir = private_dir().map_err(|e| format!("no private directory: {e}"))?;
        let socket = dir.join("s");
        let listener = UnixListener::bind(&socket).map_err(|e| format!("cannot listen: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("cannot listen: {e}"))?;
        let mut child = Command::new(exe)
            .arg("--page-scripts-child")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
        let accepted = accept_within(&listener, &mut child, Duration::from_secs(15));
        let _ = std::fs::remove_file(&socket);
        let _ = std::fs::remove_dir(&dir);
        let stream = match accepted {
            Ok(stream) => stream,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        stream
            .set_nonblocking(false)
            .map_err(|e| format!("socket: {e}"))?;
        let mut reader = stream.try_clone().map_err(|e| format!("socket: {e}"))?;
        let (events_tx, events) = channel::<Event>();
        std::thread::Builder::new()
            .name("page-scripts-reader".into())
            .spawn(move || {
                while let Ok(Some(frame)) = wire::read_frame(&mut reader) {
                    match FromChild::decode(&frame) {
                        Some(message) => {
                            if events_tx.send(Event::Message(message)).is_err() {
                                return;
                            }
                        }
                        None => break,
                    }
                }
                let _ = events_tx.send(Event::Closed);
            })
            .map_err(|e| e.to_string())?;
        let mut link = Box::new(ProcessLink {
            child,
            stream,
            events,
            synced_revision: doc.revision(),
            turn_started: None,
            failed: false,
        });
        let init = ToChild::Init {
            doc: doc.encode(),
            url: url.to_string(),
            viewport,
        };
        wire::write_frame(&mut link.stream, &init.encode())
            .map_err(|e| format!("the scripts' process went away: {e}"))?;
        match link.events.recv_timeout(Duration::from_secs(30)) {
            Ok(Event::Message(FromChild::Ready)) => {}
            Ok(Event::Message(FromChild::InitFailed(why))) => return Err(why),
            Ok(_) => return Err("the scripts' process did not say it was ready".into()),
            Err(_) => return Err("the scripts' process did not become ready".into()),
        }
        Ok(Self::with(Backend::Process(link)))
    }

    /// Whether this worker is lent the page's document, styles and boxes for each
    /// turn (a thread) rather than holding the document itself (a process).
    pub fn is_lent_state(&self) -> bool {
        matches!(self.backend, Backend::Thread { .. })
    }

    /// The scripts' process ID, for a worker that is one.
    pub fn process_id(&self) -> Option<u32> {
        match &self.backend {
            Backend::Process(link) => Some(link.child.id()),
            Backend::Thread { .. } => None,
        }
    }

    /// Whether the worker has stopped working (its process died or hung).
    pub fn failed(&self) -> bool {
        matches!(&self.backend, Backend::Process(link) if link.failed)
    }

    /// Whether a turn is in flight.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Tells a process worker the page's document as the page has it now, if the
    /// page changed it since the process last had it. The worker must be idle.
    pub fn sync_document(&mut self, doc: &Document) {
        let Backend::Process(link) = &mut self.backend else {
            return;
        };
        if link.failed || doc.revision() == link.synced_revision {
            return;
        }
        link.synced_revision = doc.revision();
        if wire::write_frame(&mut link.stream, &ToChild::Doc(doc.encode()).encode()).is_err() {
            link.failed = true;
        }
    }

    /// Starts a turn. The page must not have one in flight.
    pub fn submit(&mut self, handoff: Handoff, turn: Turn) {
        debug_assert!(!self.busy, "one turn at a time");
        self.busy = true;
        match &mut self.backend {
            Backend::Thread { jobs, .. } => {
                let _ = jobs.send(Job { handoff, turn });
            }
            Backend::Process(link) => {
                link.turn_started = Some(Instant::now());
                let rects = handoff.rects_changed.then(|| {
                    handoff
                        .rects
                        .iter()
                        .map(|(node, (x, y, w, h))| (node.as_u64(), [*x, *y, *w, *h]))
                        .collect()
                });
                let message = ToChild::Turn {
                    viewport: handoff.viewport,
                    url: handoff.url,
                    rects,
                    turn,
                };
                if wire::write_frame(&mut link.stream, &message.encode()).is_err() {
                    link.failed = true;
                }
            }
        }
    }

    /// The in-flight turn's result if it has finished. Answers the scripts'
    /// questions about the page along the way.
    pub fn try_finish(&mut self, page: &dyn PageQueries) -> Option<Done> {
        if !self.busy {
            return None;
        }
        match &mut self.backend {
            Backend::Thread { done, .. } => match done.try_recv() {
                Ok(done) => Some(self.record(done)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    self.busy = false;
                    None
                }
            },
            Backend::Process(link) => match Self::poll_link(link, page) {
                Poll::Done(done) => Some(self.process_done(done)),
                Poll::Pending => None,
                Poll::Failed => {
                    self.busy = false;
                    None
                }
            },
        }
    }

    /// Waits for the in-flight turn to finish, answering the scripts' questions
    /// about the page meanwhile.
    pub fn wait(&mut self, page: &dyn PageQueries) -> Option<Done> {
        if !self.busy {
            return None;
        }
        match &mut self.backend {
            Backend::Thread { done, .. } => match done.recv() {
                Ok(done) => Some(self.record(done)),
                Err(_) => {
                    self.busy = false;
                    None
                }
            },
            Backend::Process(link) => match Self::wait_link(link, page) {
                Poll::Done(done) => Some(self.process_done(done)),
                Poll::Pending | Poll::Failed => {
                    self.busy = false;
                    None
                }
            },
        }
    }

    /// Takes in what the process has said so far, without waiting for more.
    fn poll_link(link: &mut ProcessLink, page: &dyn PageQueries) -> Poll {
        if link.failed {
            return Poll::Failed;
        }
        loop {
            match link.events.try_recv() {
                Ok(event) => {
                    if let Some(done) = Self::process_event(link, event, page) {
                        return Poll::Done(done);
                    }
                    if link.failed {
                        return Poll::Failed;
                    }
                }
                Err(TryRecvError::Empty) => {
                    if link
                        .turn_started
                        .is_some_and(|at| at.elapsed() > HARD_TURN_LIMIT)
                    {
                        link.failed = true;
                        return Poll::Failed;
                    }
                    return Poll::Pending;
                }
                Err(TryRecvError::Disconnected) => {
                    link.failed = true;
                    return Poll::Failed;
                }
            }
        }
    }

    /// Waits for the process to finish its turn.
    fn wait_link(link: &mut ProcessLink, page: &dyn PageQueries) -> Poll {
        loop {
            if link.failed {
                return Poll::Failed;
            }
            let waited = link.turn_started.map_or(Duration::ZERO, |at| at.elapsed());
            let Some(left) = HARD_TURN_LIMIT.checked_sub(waited) else {
                link.failed = true;
                return Poll::Failed;
            };
            match link.events.recv_timeout(left) {
                Ok(event) => {
                    if let Some(done) = Self::process_event(link, event, page) {
                        return Poll::Done(done);
                    }
                }
                Err(_) => link.failed = true,
            }
        }
    }

    /// Asks a process worker for its document as it is now, with what changed in
    /// it since the page last had it. The worker must be idle.
    pub fn fetch_document(&mut self, page: &dyn PageQueries) -> Option<(Document, blueice_dom::Changes)> {
        let Backend::Process(link) = &mut self.backend else {
            return None;
        };
        if link.failed
            || wire::write_frame(&mut link.stream, &ToChild::FetchDoc.encode()).is_err()
        {
            link.failed = true;
            return None;
        }
        let started = Instant::now();
        loop {
            let left = HARD_TURN_LIMIT.checked_sub(started.elapsed())?;
            match link.events.recv_timeout(left) {
                Ok(Event::Message(FromChild::Doc { doc, changes })) => {
                    if std::env::var("BI_DIAG").is_ok() && started.elapsed().as_millis() > 10 {
                        eprintln!("DIAG fetch_document waited {:?}", started.elapsed());
                    }
                    return match (Document::decode(&doc), blueice_dom::Changes::decode(&changes)) {
                        (Some(doc), Some(changes)) => {
                            link.synced_revision = doc.revision();
                            Some((doc, changes))
                        }
                        _ => {
                            link.failed = true;
                            None
                        }
                    };
                }
                Ok(event) => {
                    Self::process_event(link, event, page);
                    if link.failed {
                        return None;
                    }
                }
                Err(_) => {
                    link.failed = true;
                    return None;
                }
            }
        }
    }

    /// Handles one thing the process said; returns the turn's result if that was it.
    fn process_event(link: &mut ProcessLink, event: Event, page: &dyn PageQueries) -> Option<ChildDone> {
        match event {
            Event::Message(FromChild::Done(done)) => Some(done),
            Event::Message(FromChild::Query(query)) => {
                let began = Instant::now();
                let answer = match query {
                    Query::Computed { node, property } => Answer::Text(
                        page.computed(NodeId::from_u64(node), &property),
                    ),
                    Query::Image(node) => Answer::Image(page.image(NodeId::from_u64(node))),
                };
                let reply = ToChild::Answer(AnswerWire(answer));
                if wire::write_frame(&mut link.stream, &reply.encode()).is_err() {
                    link.failed = true;
                }
                if std::env::var("BI_DIAG").is_ok() && began.elapsed().as_millis() > 5 {
                    eprintln!("DIAG a query took {:?} to answer", began.elapsed());
                }
                None
            }
            // Anything else out of turn means the process is not behaving.
            Event::Message(_) | Event::Closed => {
                link.failed = true;
                None
            }
        }
    }

    fn process_done(&mut self, done: ChildDone) -> Done {
        let returned = Returned {
            doc: None,
            styles: None,
            dirty: done.dirty,
            navigations: done.navigations,
            script_requests: done
                .script_requests
                .into_iter()
                .map(|(node, url)| (NodeId::from_u64(node), url))
                .collect(),
            url: done.url,
            net_jobs: done.net_jobs,
            canvases: done
                .canvases
                .into_iter()
                .map(|(node, image)| (NodeId::from_u64(node), image))
                .collect(),
            loaded_images: done
                .loaded_images
                .into_iter()
                .map(|(src, image)| {
                    let natural = image.natural();
                    (src, Arc::new(crate::images::LoadedImage { image, natural, sprite: None }))
                })
                .collect(),
        };
        self.record(Done {
            returned,
            next_timer_due: done.next_timer_due,
            clock_ms: done.clock_ms,
            sampled: Instant::now(),
            console: done.console,
        })
    }

    fn record(&mut self, done: Done) -> Done {
        self.busy = false;
        self.next_timer_due = done.next_timer_due;
        self.clock_ms = done.clock_ms;
        self.sampled = done.sampled;
        self.console.extend(done.console.iter().cloned());
        // The page keeps what a person or a test would look at, not every line a noisy
        // page ever logged.
        if self.console.len() > MAX_KEPT_CONSOLE_LINES {
            let drop = self.console.len() - MAX_KEPT_CONSOLE_LINES;
            self.console.drain(..drop);
        }
        done
    }

    /// The realm's clock now, from the last turn's reading.
    pub fn now_ms(&self) -> u64 {
        self.clock_ms + self.sampled.elapsed().as_millis() as u64
    }

    /// When the next timer is due on the realm's clock, as of the last turn.
    pub fn next_timer_due(&self) -> Option<u64> {
        self.next_timer_due
    }

    /// How long until the next timer, as of the last turn.
    pub fn until_next_timer(&self) -> Option<Duration> {
        self.next_timer_due
            .map(|due| Duration::from_millis(due.saturating_sub(self.now_ms())))
    }

    /// What the scripts have logged, up to the last finished turn.
    pub fn console(&self) -> &[String] {
        &self.console
    }
}

/// A fresh directory only this user can enter, for the one socket a child
/// connects back on.
fn private_dir() -> io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("bi-scripts-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::DirBuilder::new().mode(0o700).create(&path)?;
    Ok(path)
}

/// Waits for the child to connect, giving up if it exits first or takes too long.
fn accept_within(
    listener: &UnixListener,
    child: &mut Child,
    limit: Duration,
) -> Result<UnixStream, String> {
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(format!("accept: {e}")),
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("the scripts' process exited at once: {status}"));
        }
        if started.elapsed() > limit {
            return Err("the scripts' process never connected".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
