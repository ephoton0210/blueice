// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A document's scripts on a thread of their own.
//!
//! A [`WebHost`] is not `Send`, so it is built inside the thread that owns it
//! and never leaves. The page talks to it in *turns*: it hands over a copy of
//! the document and what layout knows, says what to run, and later gets the
//! document back with whatever the scripts changed. While a turn runs the page
//! keeps painting and answering input from the last document it got back; the
//! session is not held for as long as a script runs. Only one turn is in flight
//! at a time.

use super::dom_bridge::{DomState, NavigationRequest};
use super::turn::{run_turn, Turn};
use super::WebHost;
use blueice_dom::{Document, NodeId};
use blueice_css::ComputedStyle;
use std::collections::HashMap;
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
    pub images: Pictures,
    pub viewport: (f64, f64),
    pub url: Option<String>,
}

/// What a turn leaves behind for the page to take over.
pub(crate) struct Returned {
    pub doc: Document,
    pub styles: HashMap<NodeId, ComputedStyle>,
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

struct Job {
    handoff: Handoff,
    turn: Turn,
}

/// The page's handle on its scripts' thread.
pub(crate) struct ScriptWorker {
    jobs: Sender<Job>,
    done: Receiver<Done>,
    busy: bool,
    next_timer_due: Option<u64>,
    clock_ms: u64,
    sampled: Instant,
    console: Vec<String>,
}

impl ScriptWorker {
    /// Starts the thread and builds the realm in it for `doc`. Fails with the
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
                        doc: std::mem::replace(&mut s.doc, Document::new()),
                        styles: std::mem::take(&mut s.styles),
                        dirty: std::mem::take(&mut s.dirty),
                        navigations: std::mem::take(&mut s.navigations),
                        script_requests: std::mem::take(&mut s.script_requests),
                        url: s.url.clone(),
                        net_jobs: std::mem::take(&mut s.net_jobs),
                        canvases: s.take_canvas_pictures(),
                        loaded_images: std::mem::take(&mut s.loaded_images),
                    });
                    let console = host.console()[console_sent..].to_vec();
                    console_sent += console.len();
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
        Ok(ScriptWorker {
            jobs,
            done,
            busy: false,
            next_timer_due: None,
            clock_ms: 0,
            sampled: Instant::now(),
            console: Vec::new(),
        })
    }

    /// Whether a turn is in flight.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Starts a turn. The page must not have one in flight.
    pub fn submit(&mut self, handoff: Handoff, turn: Turn) {
        debug_assert!(!self.busy, "one turn at a time");
        self.busy = true;
        let _ = self.jobs.send(Job { handoff, turn });
    }

    /// The in-flight turn's result if it has finished.
    pub fn try_finish(&mut self) -> Option<Done> {
        if !self.busy {
            return None;
        }
        match self.done.try_recv() {
            Ok(done) => Some(self.record(done)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                None
            }
        }
    }

    /// Waits for the in-flight turn to finish.
    pub fn wait(&mut self) -> Option<Done> {
        if !self.busy {
            return None;
        }
        match self.done.recv() {
            Ok(done) => Some(self.record(done)),
            Err(_) => {
                self.busy = false;
                None
            }
        }
    }

    fn record(&mut self, done: Done) -> Done {
        self.busy = false;
        self.next_timer_due = done.next_timer_due;
        self.clock_ms = done.clock_ms;
        self.sampled = done.sampled;
        self.console.extend(done.console.iter().cloned());
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
