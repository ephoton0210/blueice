// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Running a document's own scripts. When the embedder has enabled page
//! scripts, a loaded document gets one [`WebHost`] realm: its classic scripts
//! run in document order (parser-blocking first, then `defer`), the document
//! reaches `DOMContentLoaded` and `load`, and later timers run from the
//! session tick. The realm works on the page's own `Document`, handed to it for
//! each turn and taken back afterwards, so there is one DOM and a script's
//! changes go through the ordinary restyle, layout and paint.

use super::Page;
use crate::web::{
    dom_bridge::NavigationRequest,
    turn::{js_string, ScriptPlan, Startup, Turn},
    worker::{Done, Handoff, PageQueries, ScriptWorker},
};
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// The least time between two relayouts a script's changes cause.
const WEB_RELAYOUT_INTERVAL: Duration = Duration::from_millis(250);

/// Decides whether a script's own request for one absolute URL may be made.
pub(crate) type NetworkReview = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// A page's in-flight requests: each runs on its own thread and reports here.
pub(crate) struct WebNet {
    tx: std::sync::mpsc::Sender<(u64, crate::web::net::NetOutcome)>,
    rx: std::sync::mpsc::Receiver<(u64, crate::web::net::NetOutcome)>,
    inflight: usize,
    /// Requests the page has made that are waiting for a free slot.
    queue: std::collections::VecDeque<crate::web::net::NetJob>,
}

/// How many of a page's requests run at once; the rest wait in order, as a browser's
/// connection limits make them.
const MAX_INFLIGHT_REQUESTS: usize = 16;

/// A script element of the document.
struct PageScript {
    node: NodeId,
    module: bool,
    src: Option<String>,
    inline: String,
    deferred: bool,
}

const CLASSIC_TYPES: [&str; 5] = [
    "",
    "text/javascript",
    "application/javascript",
    "text/ecmascript",
    "application/ecmascript",
];

fn collect_scripts(doc: &Document) -> Vec<PageScript> {
    fn walk(doc: &Document, node: NodeId, out: &mut Vec<PageScript>) {
        if let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            if tag_name == "script" {
                let attr = |name: &str| {
                    attributes
                        .iter()
                        .find(|(k, _)| k == name)
                        .map(|(_, v)| v.trim())
                };
                let kind = attr("type").unwrap_or("").to_ascii_lowercase();
                // Classic scripts and modules; data blocks are not run.
                let module = kind == "module";
                if (module || CLASSIC_TYPES.contains(&kind.as_str())) && attr("nomodule").is_none()
                {
                    let mut inline = String::new();
                    for child in doc.children(node) {
                        if let NodeData::Text { data } = doc.data(child) {
                            inline.push_str(data);
                        }
                    }
                    let src = attr("src").filter(|s| !s.is_empty()).map(str::to_string);
                    out.push(PageScript {
                        node,
                        module,
                        deferred: src.is_some()
                            && attr("defer").is_some()
                            && attr("async").is_none(),
                        src,
                        inline,
                    });
                }
            }
        }
        for child in doc.children(node) {
            walk(doc, child, out);
        }
    }
    let mut out = Vec::new();
    walk(doc, doc.root(), &mut out);
    out
}

/// Every laid-out box by node, in document coordinates.
fn collect_rects(
    fragment: &blueice_layout::Fragment,
    ox: f64,
    oy: f64,
    out: &mut HashMap<NodeId, (f64, f64, f64, f64)>,
) {
    let (x, y) = (ox + fragment.x, oy + fragment.y);
    if let Some(node) = fragment.node {
        out.entry(node)
            .or_insert((x, y, fragment.width, fragment.height));
    }
    for child in &fragment.children {
        collect_rects(child, x, y, out);
    }
}

/// What the page tells a process worker about its layout while a turn runs.
struct PageView<'a> {
    styles: &'a HashMap<NodeId, blueice_css::ComputedStyle>,
    images: &'a HashMap<NodeId, Arc<blueice_image::RasterImage>>,
}

impl PageQueries for PageView<'_> {
    fn computed(&self, node: NodeId, property: &str) -> Option<String> {
        crate::web::dom_bridge::computed_value_of(self.styles.get(&node)?, property)
    }
    fn image(&self, node: NodeId) -> Option<Arc<blueice_image::RasterImage>> {
        self.images.get(&node).cloned()
    }
}

impl Page {
    fn web_scripts_on(&self) -> bool {
        self.web_scripts_override
            .unwrap_or_else(crate::web_scripts_enabled)
    }

    /// Test and embedder hook: run (or not) this page's scripts regardless of
    /// the process setting.
    #[cfg(test)]
    pub(crate) fn set_web_scripts(&mut self, enabled: bool) {
        self.web_scripts_override = Some(enabled);
    }

    /// What a script's own network requests are checked against. Without one,
    /// every request is refused. (A session sets it from the gatekeeper
    /// clearance; tests and tools set it directly.)
    #[cfg(test)]
    pub(crate) fn set_network_review(&mut self, review: NetworkReview) {
        self.web_review = Some(review);
    }

    /// Waits (briefly) for put-off script relayouts to happen.
    #[cfg(test)]
    pub(crate) fn settle(&mut self) {
        let until = std::time::Instant::now() + Duration::from_secs(2);
        while self.web_relayout_pending && std::time::Instant::now() < until {
            self.tick_web();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A script scrolled the window to `y` (document pixels from the top): the page goes there,
    /// within what can be scrolled, and the new position is a frame to publish -- the human and
    /// anything reading the representation see it in the same render pass as any other scroll.
    /// The scripts hear of it as a scroll event on the next tick, like a wheel scroll.
    fn scroll_from_script(&mut self, y: f64) {
        let max_scroll = (self.fragment.height - self.viewport_height).max(0.0);
        let y = y.clamp(0.0, max_scroll);
        if (y - self.scroll_y).abs() < f64::EPSILON {
            return;
        }
        self.scroll_y = y;
        self.script_change_unpublished = true;
        self.script_change_unpublished_by_web = true;
    }

    pub(crate) fn take_web_navigation(&mut self) -> Option<String> {
        self.web_navigation.take()
    }

    /// Whether this page's scripts run without holding the session: a timer,
    /// scroll or network turn is started and its result taken when it is done,
    /// while the page keeps answering from the document it has.
    fn web_async_on(&self) -> bool {
        self.web_async_override
            .unwrap_or_else(crate::async_scripts_enabled)
    }

    /// Test and embedder hook: run turns in the background (or not) regardless
    /// of the process setting.
    #[cfg(test)]
    pub(crate) fn set_async_scripts(&mut self, enabled: bool) {
        self.web_async_override = Some(enabled);
    }

    /// The page's DOM, styles and layout boxes, lent to a turn. `lend` moves
    /// the document out (the page waits for it); otherwise the scripts get a
    /// copy and the page keeps painting from its own.
    fn web_handoff(&mut self, lend: bool) -> Handoff {
        let fonts = (self.linked_fonts.generation() != self.web_fonts_sent)
            .then(|| self.linked_fonts.script_view());
        self.web_fonts_sent = self.linked_fonts.generation();
        let lent_state = self.web.as_ref().is_none_or(ScriptWorker::is_lent_state);
        if !lent_state {
            // A process holds its own document and asks about styles and pictures when it
            // needs to; it is sent the boxes once per layout.
            let rects_changed = self.layout_generation != self.web_rects_sent;
            let rects = if rects_changed {
                self.ensure_web_rects();
                self.web_rects_sent = self.layout_generation;
                self.web_rects.clone().unwrap_or_default()
            } else {
                HashMap::new()
            };
            return Handoff {
                doc: Document::new(),
                styles: HashMap::new(),
                rects,
                rects_changed,
                images: HashMap::new(),
                viewport: (self.viewport_width, self.viewport_height),
                url: self.url.clone(),
                fonts,
            };
        }
        let mut rects = HashMap::new();
        collect_rects(&self.fragment, 0.0, 0.0, &mut rects);
        let (doc, styles) = if lend {
            (
                std::mem::replace(&mut self.doc, Document::new()),
                std::mem::take(&mut self.styles),
            )
        } else {
            (self.doc.clone(), self.styles.clone())
        };
        Handoff {
            doc,
            styles,
            rects,
            rects_changed: true,
            images: self.images.content.clone(),
            viewport: (self.viewport_width, self.viewport_height),
            url: self.url.clone(),
            fonts,
        }
    }

    /// Every laid-out box by node, kept until the next layout.
    fn ensure_web_rects(&mut self) {
        if self.web_rects.is_none() {
            let mut rects = HashMap::new();
            collect_rects(&self.fragment, 0.0, 0.0, &mut rects);
            self.web_rects = Some(rects);
        }
    }

    /// Ends the page's scripts if their process has stopped working: the page
    /// goes on without them, as a page whose scripts all threw would.
    fn drop_failed_scripts(&mut self) {
        if self.web.as_ref().is_some_and(ScriptWorker::failed) {
            if std::env::var("BI_DIAG").is_ok() || std::env::var_os("BI_NET_LOG").is_some() {
                eprintln!("page scripts: the scripts' process stopped working; the page goes on without them");
            }
            self.web = None;
            self.web_scripts_lost = true;
        }
    }

    /// Runs `f` on the realm with the page's DOM, styles and layout boxes
    /// handed over and waits for it, then takes them back; a changed DOM is
    /// restyled and laid out and marked for publication. Returns the external
    /// scripts the turn asked to load.
    fn web_turn_once(&mut self, turn: Turn) -> (Vec<(NodeId, String)>, bool) {
        let Some(mut worker) = self.web.take() else {
            return (Vec::new(), false);
        };
        // A background turn still running must be taken first.
        let view = PageView {
            styles: &self.styles,
            images: &self.images.content,
        };
        let earlier = worker.wait(&view);
        let mut earlier_result = (Vec::new(), false);
        self.web = Some(worker);
        if let Some(done) = earlier {
            earlier_result = self.finish_turn(done, false);
        }
        let Some(mut worker) = self.web.take() else {
            return earlier_result;
        };
        worker.sync_document(&self.doc);
        let handoff = {
            self.web = Some(worker);
            let handoff = self.web_handoff(true);
            worker = self.web.take().expect("put back just above");
            handoff
        };
        worker.submit(handoff, turn);
        let view = PageView {
            styles: &self.styles,
            images: &self.images.content,
        };
        let done = worker.wait(&view);
        self.web = Some(worker);
        match done {
            Some(done) => {
                let (mut requests, relayed) = self.finish_turn(done, true);
                let mut earlier_requests = earlier_result.0;
                earlier_requests.append(&mut requests);
                (earlier_requests, relayed || earlier_result.1)
            }
            None => {
                // The scripts' thread or process is gone; the page goes on without them.
                self.web = None;
                self.web_scripts_lost = true;
                earlier_result
            }
        }
    }

    /// Starts `f` as a turn that the page does not wait for. Nothing is started
    /// while an earlier turn is still running.
    fn web_turn_in_background(&mut self, turn: Turn) -> bool {
        if self.web.as_ref().is_none_or(ScriptWorker::busy) {
            return false;
        }
        let started = std::time::Instant::now();
        let handoff = self.web_handoff(false);
        if std::env::var("BI_DIAG").is_ok() && started.elapsed().as_millis() > 20 {
            eprintln!("DIAG handoff copy took {:?}", started.elapsed());
        }
        if let Some(worker) = self.web.as_mut() {
            worker.sync_document(&self.doc);
            worker.submit(handoff, turn);
        }
        true
    }

    /// Starts `f` in the background when the page's scripts run that way, and
    /// otherwise runs it and waits.
    fn web_turn_auto(&mut self, turn: Turn) {
        if self.web_async_on() {
            self.web_turn_in_background(turn);
        } else {
            self.web_turn(turn);
        }
    }

    /// Takes the result of a turn that has finished: the document comes back
    /// with whatever the scripts did to it, and what they asked for is started.
    /// `lent` is whether the page's own document went out with the turn (so its
    /// styles come back too); a turn run on a copy leaves the page's styles.
    fn finish_turn(&mut self, done: Done, lent: bool) -> (Vec<(NodeId, String)>, bool) {
        let mut r = done.returned;
        let scroll_to = r.scroll_to;
        let fonts = std::mem::take(&mut r.loaded_fonts);
        let (dirty, navigations, requests, url, jobs, canvases, loaded) = (
            r.dirty,
            r.navigations,
            r.script_requests,
            r.url,
            r.net_jobs,
            r.canvases,
            r.loaded_images,
        );
        let mut adopted = r.doc.map(|doc| (doc, None));
        if adopted.is_none() && dirty {
            // The scripts hold the document; take it now that they changed it.
            let view = PageView {
                styles: &self.styles,
                images: &self.images.content,
            };
            if let Some(worker) = self.web.as_mut() {
                adopted = worker
                    .fetch_document(&view)
                    .map(|(doc, changes)| (doc, Some(changes)));
            }
        }
        if let Some((mut doc, changes)) = adopted {
            if let Some(changes) = changes {
                // What the page itself changed and has not restyled for yet, then
                // what the scripts changed: the restyle sees both across the swap.
                doc.absorb_changes(self.doc.take_changes());
                doc.absorb_changes(changes);
            }
            self.doc = doc;
        }
        if let Some(styles) = r.styles.filter(|_| lent) {
            self.styles = styles;
        }
        self.drop_failed_scripts();
        self.start_net_jobs(jobs);
        for navigation in navigations {
            match navigation {
                NavigationRequest::PushState { .. } => self.url = Some(url.clone()),
                NavigationRequest::Load { url, .. } => self.web_navigation = Some(url),
                NavigationRequest::Reload => {
                    if let Some(url) = self.url.clone() {
                        self.web_navigation = Some(url);
                    }
                }
                NavigationRequest::Go(_) => {}
            }
        }
        if let Some(y) = scroll_to {
            self.scroll_from_script(y);
        }
        let added = fonts
            .iter()
            .filter(|font| self.linked_fonts.add_script_font(font))
            .count();
        if added > 0 {
            self.web_fonts_changed = true;
            self.web_relayout_pending = true;
        }
        if !loaded.is_empty() {
            for (src, image) in loaded {
                self.linked_images.insert(src, image);
            }
            self.web_relayout_pending = true;
        }
        for (node, image) in canvases {
            let natural = (f64::from(image.width), f64::from(image.height));
            let first = !self.canvas_pictures.contains_key(&node);
            self.canvas_pictures.insert(
                node,
                Arc::new(crate::images::LoadedImage { image, natural, sprite: None }),
            );
            if first {
                self.web_relayout_pending = true;
            } else {
                self.refresh_canvas_images();
                self.script_change_unpublished = true;
                self.script_change_unpublished_by_web = true;
            }
        }
        if dirty {
            self.web_relayout_pending = true;
        }
        let relayed = self.relayout_for_scripts(false);
        (requests, relayed)
    }

    /// Restyles and lays the page out if a script changed it and either enough
    /// time has passed since the last time or `force` (or a load in progress)
    /// asks. Returns whether it did.
    fn relayout_for_scripts(&mut self, force: bool) -> bool {
        if !self.web_relayout_pending {
            return false;
        }
        let waited = self
            .web_last_relayout
            .is_none_or(|at| at.elapsed() >= WEB_RELAYOUT_INTERVAL);
        if !(force || self.web_force_relayout || waited) {
            return false;
        }
        let started = std::time::Instant::now();
        if std::mem::take(&mut self.web_fonts_changed) {
            self.recascade();
            self.relayout_keeping_cache();
        } else {
            self.restyle_and_relayout();
        }
        if std::env::var("BI_DIAG").is_ok() {
            eprintln!(
                "DIAG restyle+relayout {:?} (incremental {}, full {})",
                started.elapsed(),
                self.restyle_counts.0,
                self.restyle_counts.1
            );
        }
        self.web_relayout_pending = false;
        self.web_last_relayout = Some(std::time::Instant::now());
        self.script_change_unpublished = true;
        self.script_change_unpublished_by_web = true;
        true
    }

    /// One turn, then the scripts it inserted: each fetched under the same
    /// review as any request and run (or failed) as its own turn. A bounded
    /// number of rounds, so scripts that insert scripts cannot run forever.
    fn web_turn(&mut self, turn: Turn) {
        let (pending, changed) = self.web_turn_once(turn);
        self.web_followups(pending, changed);
    }

    /// What a finished turn leaves to do: the scripts it inserted, then the
    /// observers that report on layout once it has caught up.
    fn web_followups(&mut self, mut pending: Vec<(NodeId, String)>, mut changed: bool) {
        for _ in 0..16 {
            if pending.is_empty() {
                break;
            }
            let mut next = Vec::new();
            for (node, src) in std::mem::take(&mut pending) {
                let source = self.web_review.as_ref().and_then(|review| {
                    crate::scripts_fetch::fetch_one(
                        &src,
                        &**review,
                        crate::scripts_fetch::DEFAULT_SCRIPT_LIMITS,
                    )
                });
                // Developer aid: keep the scripts the page loaded after it started (`BI_SAVE`).
                if let (Ok(dir), Some(text)) = (std::env::var("BI_SAVE"), source.as_ref()) {
                    let _ = std::fs::create_dir_all(format!("{dir}/loaded"));
                    let name: String = src
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' })
                        .rev()
                        .take(120)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    let _ = std::fs::write(format!("{dir}/loaded/{name}"), text);
                }
                let trace = std::env::var_os("BI_NET_LOG").is_some();
                if trace {
                    eprintln!(
                        "script-load {src} -> {}",
                        source.as_ref().map_or("not fetched".to_string(), |s| format!(
                            "{} bytes",
                            s.len()
                        ))
                    );
                }
                let (more, dirty) = self.web_turn_once(Turn::ScriptLoaded {
                    node: node.as_u64(),
                    src,
                    source,
                });
                changed |= dirty;
                next.extend(more);
            }
            pending = next;
        }
        // A change of the document moves things, which observers of visibility
        // report on once layout has caught up; their callbacks may change it
        // again, a few rounds at most.
        if self.web_async_on() {
            // The scripts run in the background: let them report on layout when they
            // next have a turn, without holding the session for it.
            if changed {
                self.web_after_layout = 3;
            }
            return;
        }
        for _ in 0..3 {
            if !changed {
                break;
            }
            let (_, dirty) = self.web_turn_once(Turn::AfterLayout);
            changed = dirty;
        }
    }

    /// Hands requests a script made to worker threads. Each is reviewed hop by
    /// hop on its thread; without a review nothing leaves the page.
    fn start_net_jobs(&mut self, jobs: Vec<crate::web::net::NetJob>) {
        if jobs.is_empty() {
            return;
        }
        let net = self.web_net.get_or_insert_with(|| {
            let (tx, rx) = std::sync::mpsc::channel();
            WebNet {
                tx,
                rx,
                inflight: 0,
                queue: std::collections::VecDeque::new(),
            }
        });
        net.queue.extend(jobs);
        self.pump_net_jobs();
    }

    /// Starts queued requests while there are free slots.
    fn pump_net_jobs(&mut self) {
        let Some(net) = self.web_net.as_mut() else {
            return;
        };
        while net.inflight < MAX_INFLIGHT_REQUESTS {
            let Some(job) = net.queue.pop_front() else {
                break;
            };
            net.inflight += 1;
            let tx = net.tx.clone();
            let review = self.web_review.clone();
            std::thread::spawn(move || {
                let outcome = match review {
                    Some(review) => crate::web::net::perform(&job, &*review),
                    None => crate::web::net::NetOutcome::Failed(
                        "network access is not enabled for this page".to_string(),
                    ),
                };
                let _ = tx.send((job.id, outcome));
            });
        }
    }

    /// Delivers finished requests to the page's scripts.
    fn deliver_net_results(&mut self) {
        let Some(net) = self.web_net.as_mut() else {
            return;
        };
        let mut done = Vec::new();
        while let Ok(result) = net.rx.try_recv() {
            net.inflight = net.inflight.saturating_sub(1);
            done.push(result);
        }
        // Slots freed by the requests that finished go to the ones waiting.
        self.pump_net_jobs();
        let Some(_) = self.web_net.as_mut() else {
            return;
        };
        let mut calls = Vec::new();
        for (id, outcome) in done {
            let call = match outcome {
                crate::web::net::NetOutcome::Done {
                    status,
                    status_text,
                    headers,
                    body,
                    url,
                } => {
                    let pairs: Vec<(String, String)> = headers;
                    format!(
                        "__blueice.netDone({id}, {status}, {}, {}, \"{}\", {});",
                        js_string(&status_text),
                        js_string(&serde_json::to_string(&pairs).unwrap_or_else(|_| "[]".into())),
                        crate::web::net::base64_encode(&body),
                        js_string(&url),
                    )
                }
                crate::web::net::NetOutcome::Failed(message) => {
                    format!("__blueice.netFail({id}, {});", js_string(&message))
                }
            };
            calls.push(call);
        }
        if !calls.is_empty() {
            // One turn for everything that arrived.
            self.web_turn_auto(Turn::NetCalls(calls));
        }
    }

    /// Starts this document's scripts, if they are enabled: the realm, every
    /// classic script, then `DOMContentLoaded` and `load`.
    pub(super) fn start_web_scripts(&mut self) {
        self.web = None;
        self.web_scripts_lost = false;
        self.web_rects_sent = 0;
        self.web_after_layout = 0;
        self.web_in_flight_after_layout = false;
        self.web_relayout_pending = false;
        self.web_last_relayout = None;
        if !self.web_scripts_on() {
            return;
        }
        let scripts = collect_scripts(&self.doc);
        // The realm is built against (a copy of) the real document: its root
        // node's id is part of what the platform captures.
        let created = std::time::Instant::now();
        let url = self.url.clone().unwrap_or_else(|| "about:blank".to_string());
        let viewport = (self.viewport_width, self.viewport_height);
        let spawned = match crate::script_process_exe() {
            Some(exe) => ScriptWorker::spawn_process(&exe, &self.doc, &url, viewport).or_else(
                |error| {
                    // Scripts on a thread are better than none; say so.
                    eprintln!(
                        "page scripts: the scripts' process could not start ({error}); running them on a thread"
                    );
                    ScriptWorker::spawn(self.doc.clone(), &url, viewport)
                },
            ),
            None => ScriptWorker::spawn(self.doc.clone(), &url, viewport),
        };
        let worker = match spawned {
            Ok(worker) => worker,
            Err(error) => {
                if std::env::var("BI_DIAG").is_ok() {
                    eprintln!("page scripts could not start: {error}");
                }
                return;
            }
        };
        if std::env::var("BI_DIAG").is_ok() {
            eprintln!("DIAG realm created in {:?}", created.elapsed());
        }
        self.web = Some(worker);
        self.web_fonts_sent = 0;
        self.web_force_relayout = true;
        self.web_turn(Turn::Startup(Startup {
            scripts: scripts
                .into_iter()
                .map(|s| ScriptPlan {
                    node: s.node.as_u64(),
                    module: s.module,
                    src: s.src,
                    inline: s.inline,
                    deferred: s.deferred,
                })
                .collect(),
            linked: self.linked_scripts.clone(),
            modules: self.linked_modules.clone(),
            document: self.url.clone(),
        }));
        self.web_force_relayout = false;
    }

    /// Runs the timers that are due. True when the document changed.
    pub(crate) fn tick_web(&mut self) -> bool {
        let ticked = std::time::Instant::now();
        let published = self.tick_web_inner();
        if ticked.elapsed().as_millis() > 30 && std::env::var("BI_DIAG").is_ok() {
            eprintln!("DIAG tick_web held the session for {:?}", ticked.elapsed());
        }
        published
    }

    fn tick_web_inner(&mut self) -> bool {
        let background = self.web_async_on();
        let mut published = false;
        if background {
            // A turn that finished since the last tick comes back first, and only
            // then is another started: the scripts have one turn in flight.
            let finished = {
                let view = PageView {
                    styles: &self.styles,
                    images: &self.images.content,
                };
                self.web.as_mut().and_then(|worker| worker.try_finish(&view))
            };
            self.drop_failed_scripts();
            if let Some(done) = finished {
                let rounds_left = self.web_after_layout;
                let was_after_layout = std::mem::take(&mut self.web_in_flight_after_layout);
                let (requests, relayed) = self.finish_turn(done, false);
                self.web_followups(requests, relayed);
                if was_after_layout {
                    // A report on layout that changed the page does not start the count over.
                    self.web_after_layout = rounds_left;
                }
                published = std::mem::take(&mut self.script_change_unpublished_by_web);
            }
            if self.web.as_ref().is_none_or(ScriptWorker::busy) {
                return published;
            }
        }
        if background && self.web_after_layout > 0 {
            self.web_after_layout -= 1;
            if self.web_turn_in_background(Turn::AfterLayout) {
                self.web_in_flight_after_layout = true;
            }
            return published;
        }
        // Scrolling reaches the page's scripts as a scroll event and a fresh
        // look at what is visible.
        if self.web.is_some() && (self.scroll_y - self.web_scroll_seen).abs() > 0.5 {
            self.web_scroll_seen = self.scroll_y;
            let y = self.scroll_y;
            self.web_turn_auto(Turn::Scrolled(y));
            if background {
                return published;
            }
        }
        self.deliver_net_results();
        if background && self.web.as_ref().is_none_or(ScriptWorker::busy) {
            return published;
        }
        self.relayout_for_scripts(false);
        let due = self
            .web
            .as_ref()
            .and_then(|worker| worker.next_timer_due().map(|due| due <= worker.now_ms()))
            .unwrap_or(false);
        if !due {
            return published;
        }
        self.web_turn_auto(Turn::Tick);
        if background {
            return published;
        }
        std::mem::take(&mut self.script_change_unpublished_by_web)
    }

    /// How long until this document's scripts next have work, if ever.
    pub(crate) fn web_wake_in(&self) -> Option<Duration> {
        let worker = self.web.as_ref()?;
        // While requests are out, look for their answers often; while a turn
        // runs in the background, look for its result.
        let waiting = (self.web_net.as_ref().is_some_and(|net| net.inflight > 0)
            || worker.busy())
        .then_some(Duration::from_millis(10));
        let relayout = self.web_relayout_pending.then(|| {
            WEB_RELAYOUT_INTERVAL.saturating_sub(
                self.web_last_relayout
                    .map_or(Duration::ZERO, |at| at.elapsed()),
            )
        });
        let waiting = match (waiting, relayout) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let timer = worker.until_next_timer();
        match (waiting, timer) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Where this page's scripts run: `"process"`, `"thread"`, `"lost"` (their
    /// process stopped working) or `"none"`.
    #[doc(hidden)]
    pub fn script_backend(&self) -> &'static str {
        match &self.web {
            Some(worker) if worker.is_lent_state() => "thread",
            Some(_) => "process",
            None if self.web_scripts_lost => "lost",
            None => "none",
        }
    }

    /// Changes the page's own copy of the document the way a native edit does (typing
    /// into a field, say), for tests of what scripts see afterwards.
    #[doc(hidden)]
    pub fn edit_document(&mut self, edit: impl FnOnce(&mut Document)) {
        edit(&mut self.doc);
        self.web_relayout_pending = true;
    }

    /// The ID of the process running this page's scripts, if they run in one.
    #[doc(hidden)]
    pub fn script_process_id(&self) -> Option<u32> {
        self.web.as_ref().and_then(ScriptWorker::process_id)
    }

    /// Runs the page's script work that is due (what a session does on each
    /// tick); true when the document changed.
    #[doc(hidden)]
    pub fn tick_scripts(&mut self) -> bool {
        self.tick_web()
    }

    /// Runs `source` in the page's realm as a diagnostic probe.
    #[allow(dead_code)]
    pub(crate) fn web_eval(&mut self, source: &str) {
        self.web_turn(Turn::Probe(source.to_string()));
    }

    /// What the realm's scripts have logged (diagnostics).
    #[allow(dead_code)]
    pub(crate) fn web_console(&self) -> Vec<String> {
        self.web
            .as_ref()
            .map(|worker| worker.console().to_vec())
            .unwrap_or_default()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use blueice_paint::PaintCommand;

    fn page(html: &str) -> Page {
        let mut page = Page::new(400.0, 300.0);
        page.set_web_scripts(true);
        page.load_html_str(html, Some("https://example.com/p".to_string()));
        page
    }

    fn texts(page: &Page) -> Vec<String> {
        page.render()
            .commands
            .iter()
            .filter_map(|c| match c {
                PaintCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// 3000 px of page: a 100 px header, then boxes `a` (at 1000), `b` (at 2000) and a tail.
    const TALL: &str = "<body style='margin:0'><div style='height:1000px'>top</div>\
        <div id=a style='height:200px'>a</div><div style='height:800px'></div>\
        <div id=b style='height:100px'>b</div><div style='height:900px'></div></body>";

    fn scrolled_by(script: &str) -> f64 {
        let mut page = page(TALL);
        page.web_eval(script);
        page.settle();
        page.scroll_y()
    }

    #[test]
    fn a_page_script_can_scroll_the_window() {
        assert_eq!(scrolled_by("window.scrollTo(0, 500)"), 500.0);
        assert_eq!(scrolled_by("window.scrollTo({ top: 700, behavior: 'smooth' })"), 700.0);
        assert_eq!(scrolled_by("window.scroll(0, 250)"), 250.0);
        assert_eq!(scrolled_by("window.scrollTo(0, 100); window.scrollBy(0, 50)"), 150.0);
        assert_eq!(scrolled_by("window.scrollBy({ top: 80, left: 5 })"), 80.0);
        assert_eq!(scrolled_by("document.documentElement.scrollTop = 321"), 321.0);
        assert_eq!(scrolled_by("document.scrollingElement.scrollTop = 99"), 99.0);
        assert_eq!(scrolled_by("document.body.scrollTop = 42"), 42.0);
        // The end of the page, and not past it; not before the start; nonsense is ignored.
        assert_eq!(scrolled_by("window.scrollTo(0, 99999)"), 2700.0);
        assert_eq!(scrolled_by("window.scrollTo(0, 300); window.scrollTo(0, -50)"), 0.0);
        assert_eq!(scrolled_by("window.scrollTo(0, 300); window.scrollTo(0, NaN)"), 300.0);
        assert_eq!(scrolled_by("window.scrollTo(0, 300); window.scrollTo()"), 300.0);
    }

    #[test]
    fn scroll_position_reads_back_in_the_same_script() {
        let mut page = page(
            "<body style='margin:0'><div style='height:3000px'></div><p id=out>x</p></body>",
        );
        page.web_eval(
            "window.scrollTo(0, 640);\
             document.getElementById('out').textContent = [window.scrollY, window.pageYOffset, document.documentElement.scrollTop].join(',');",
        );
        page.settle();
        assert!(texts(&page).iter().any(|t| t == "640,640,640"), "{:?}", texts(&page));
    }

    #[test]
    fn scroll_into_view_aligns_the_element() {
        // The viewport is 300 px tall: `a` is 200 px at 1000, `b` 100 px at 2000.
        assert_eq!(scrolled_by("document.getElementById('a').scrollIntoView()"), 1000.0);
        assert_eq!(scrolled_by("document.getElementById('a').scrollIntoView(true)"), 1000.0);
        assert_eq!(scrolled_by("document.getElementById('a').scrollIntoView({ block: 'start' })"), 1000.0);
        assert_eq!(scrolled_by("document.getElementById('a').scrollIntoView({ block: 'center' })"), 950.0);
        assert_eq!(scrolled_by("document.getElementById('a').scrollIntoView({ block: 'end' })"), 900.0);
        assert_eq!(scrolled_by("document.getElementById('a').scrollIntoView(false)"), 900.0);
        // `nearest` moves only as far as needed: from the top, `b` comes in at the bottom edge.
        assert_eq!(scrolled_by("document.getElementById('b').scrollIntoView({ block: 'nearest' })"), 1800.0);
        // ...and not at all when it is already showing.
        assert_eq!(
            scrolled_by(
                "window.scrollTo(0, 1900); document.getElementById('b').scrollIntoView({ block: 'nearest' })"
            ),
            1900.0
        );
        // The last element cannot be scrolled to the top: the page ends first.
        assert_eq!(scrolled_by("document.getElementById('b').scrollIntoView()"), 2000.0);
    }

    #[test]
    fn a_scripted_scroll_is_a_scroll_event_a_new_frame_and_a_fresh_look_at_what_is_visible() {
        let mut page = page(
            "<body style='margin:0'><div style='height:1500px'>top</div><p id=out>0</p><script>\
             let n = 0;\
             window.addEventListener('scroll', () => { n += 1; document.getElementById('out').textContent = 'scrolls ' + n + ' at ' + window.scrollY; });\
             </script></body>",
        );
        let _ = page.take_unpublished_script_change();
        page.web_eval("window.scrollTo(0, 600)");
        page.settle();
        for _ in 0..20 {
            page.tick_web();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(page.scroll_y(), 600.0);
        assert!(page.take_unpublished_script_change(), "the scroll is a frame to publish");
        let shown = texts(&page).join(" ");
        assert!(shown.contains("scrolls 1 at 600"), "one scroll event, seeing the new position: {shown}");
    }

    fn background_page(html: &str) -> Page {
        let mut page = Page::new(400.0, 300.0);
        page.set_web_scripts(true);
        page.set_async_scripts(true);
        page.load_html_str(html, Some("https://example.com/p".to_string()));
        page
    }

    const SLOW_TIMER: &str = "<body><p id=a>old</p><script>\
        setTimeout(function () { var end = Date.now() + 600; while (Date.now() < end) {} \
          document.getElementById('a').textContent = 'new'; }, 0);</script></body>";

    #[test]
    fn a_long_timer_turn_in_the_background_does_not_hold_the_page() {
        let mut page = background_page(SLOW_TIMER);
        std::thread::sleep(Duration::from_millis(20));
        let begun = std::time::Instant::now();
        page.tick_web();
        assert!(
            begun.elapsed() < Duration::from_millis(300),
            "starting the turn returned at once: {:?}",
            begun.elapsed()
        );
        // The script is still spinning; the page paints and scrolls from what it has.
        let asked = std::time::Instant::now();
        page.scroll_by(10.0);
        assert_eq!(texts(&page), vec!["old"]);
        assert!(
            asked.elapsed() < Duration::from_millis(200),
            "painting did not wait for the script: {:?}",
            asked.elapsed()
        );
        // Its result arrives when the turn is done.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while texts(&page) != vec!["new"] && std::time::Instant::now() < deadline {
            page.tick_web();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(texts(&page), vec!["new"]);
        assert!(
            begun.elapsed() >= Duration::from_millis(500),
            "the turn really took its 600 ms: {:?}",
            begun.elapsed()
        );
    }

    #[test]
    fn without_background_turns_the_same_tick_holds_the_page() {
        let mut page = page(SLOW_TIMER);
        std::thread::sleep(Duration::from_millis(20));
        let begun = std::time::Instant::now();
        page.tick_web();
        assert!(
            begun.elapsed() >= Duration::from_millis(500),
            "the tick waited for the script: {:?}",
            begun.elapsed()
        );
        assert_eq!(texts(&page), vec!["new"]);
    }

    #[test]
    fn background_timers_chain_one_turn_after_another() {
        let mut page = background_page(
            "<body><p id=a>0</p><script>let n = 0; (function again() { n += 1; \
             document.getElementById('a').textContent = String(n); if (n < 4) setTimeout(again, 0); })();\
             </script></body>",
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while texts(&page) != vec!["4"] && std::time::Instant::now() < deadline {
            page.tick_web();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(texts(&page), vec!["4"]);
    }

    #[test]
    fn leaving_a_page_with_a_turn_running_does_not_wait_for_it() {
        let mut page = background_page(SLOW_TIMER);
        std::thread::sleep(Duration::from_millis(20));
        page.tick_web();
        let begun = std::time::Instant::now();
        page.load_html_str("<body><p>next</p></body>", Some("https://example.com/n".into()));
        assert!(
            begun.elapsed() < Duration::from_millis(300),
            "loading the next document did not wait: {:?}",
            begun.elapsed()
        );
        assert_eq!(texts(&page), vec!["next"]);
    }

    #[test]
    fn the_root_elements_client_size_is_the_window_and_the_screen_is_at_least_the_window() {
        // Scripts read `document.documentElement.clientHeight` as the height of the window
        // (the page can be far taller), and a screen is never smaller than the window on it.
        let page = page(
            "<body style='margin:0'><div style='height:3000px'>tall</div><p id=out>x</p><script>\
             const de = document.documentElement;\
             document.getElementById('out').textContent = [de.clientWidth === innerWidth, \
               de.clientHeight === innerHeight, document.body.clientHeight > innerHeight, \
               de.scrollHeight >= 3000, screen.width >= innerWidth, screen.height >= innerHeight, \
               screen.availHeight >= innerHeight].join(',');</script></body>",
        );
        assert!(
            texts(&page).iter().any(|t| t == "true,true,true,true,true,true,true"),
            "{:?}",
            texts(&page)
        );
    }

    #[test]
    fn inline_scripts_run_in_order_and_their_changes_are_painted() {
        let page = page(
            "<body><p id=a>old</p><script>document.getElementById('a').textContent = 'one';</script>\
             <script>document.getElementById('a').textContent += ' two';</script></body>",
        );
        assert_eq!(texts(&page), vec!["one", "two"]);
    }

    #[test]
    fn a_webgl_canvas_is_drawn_by_script_and_painted_into_the_page() {
        let mut page = page(
            "<body style='margin:0'><canvas id=c width=40 height=20></canvas><script>\
             const gl = document.getElementById('c').getContext('webgl');\
             const sh = (t, src) => { const s = gl.createShader(t); gl.shaderSource(s, src); gl.compileShader(s); \
               if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); return s; };\
             const p = gl.createProgram();\
             gl.attachShader(p, sh(gl.VERTEX_SHADER, 'attribute vec2 a; void main() { gl_Position = vec4(a, 0.0, 1.0); }'));\
             gl.attachShader(p, sh(gl.FRAGMENT_SHADER, 'precision mediump float; uniform vec4 c; void main() { gl_FragColor = c; }'));\
             gl.linkProgram(p); gl.useProgram(p);\
             gl.uniform4f(gl.getUniformLocation(p, 'c'), 0, 1, 0, 1);\
             const b = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, b);\
             gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 3,-1, -1,3]), gl.STATIC_DRAW);\
             const l = gl.getAttribLocation(p, 'a'); gl.enableVertexAttribArray(l);\
             gl.vertexAttribPointer(l, 2, gl.FLOAT, false, 0, 0);\
             gl.clearColor(0, 0, 1, 1); gl.clear(gl.COLOR_BUFFER_BIT);\
             gl.drawArrays(gl.TRIANGLES, 0, 3);\
             </script></body>",
        );
        page.settle();
        let pixmap = page.render_visible();
        let px = |x: usize, y: usize| {
            let i = (y * pixmap.width as usize + x) * 4;
            [pixmap.pixels[i], pixmap.pixels[i + 1], pixmap.pixels[i + 2]]
        };
        assert_eq!(
            px(20, 10),
            [0, 255, 0],
            "the canvas shows what the script drew"
        );
    }

    #[test]
    fn a_2d_canvas_can_be_drawn_on_read_back_and_is_painted() {
        let mut page = page(
            "<body style='margin:0'><canvas id=c width=40 height=20></canvas><script>\
             const c = document.getElementById('c'); const g = c.getContext('2d');\
             g.fillStyle = '#ff0000'; g.fillRect(0, 0, 40, 20);\
             g.fillStyle = 'rgb(0, 0, 255)'; g.fillRect(10, 5, 10, 10);\
             const d = g.getImageData(12, 8, 1, 1).data;\
             const copy = document.createElement('canvas'); copy.width = 4; copy.height = 4;\
             copy.getContext('2d').drawImage(c, 10, 5, 2, 2, 0, 0, 4, 4);\
             const e = copy.getContext('2d').getImageData(1, 1, 1, 1).data;\
             console.log('RESULT ' + [d[0], d[1], d[2], d[3], e[0], e[1], e[2], e[3]].join(',') + ' ' + c.toDataURL().slice(0, 22));\
             </script></body>",
        );
        page.settle();
        let pixmap = page.render_visible();
        let px = |x: usize, y: usize| {
            let i = (y * pixmap.width as usize + x) * 4;
            [pixmap.pixels[i], pixmap.pixels[i + 1], pixmap.pixels[i + 2]]
        };
        assert_eq!(px(2, 2), [255, 0, 0], "the red background");
        assert_eq!(px(15, 10), [0, 0, 255], "the blue square");
        let log = page.web_console().join("\n");
        assert!(
            log.contains("RESULT 0,0,255,255,0,0,255,255 data:image/png;base64,"),
            "{log}"
        );
    }

    #[test]
    fn a_script_created_image_loads_fires_load_and_can_be_read_through_a_canvas() {
        // A 2x1 PNG: red, blue.
        let png = blueice_image::encode_png(&blueice_image::RasterImage::new(
            2,
            1,
            vec![255, 0, 0, 255, 0, 0, 255, 255],
        ))
        .unwrap();
        let url = format!(
            "data:image/png;base64,{}",
            crate::web::net::base64_encode(&png)
        );
        let mut page = page(&format!(
            "<body><script>\
             const img = new Image();\
             img.onload = () => {{\
               const c = document.createElement('canvas'); c.width = 2; c.height = 1;\
               const g = c.getContext('2d'); g.drawImage(img, 0, 0);\
               const d = g.getImageData(1, 0, 1, 1).data;\
               console.log('RESULT ' + img.naturalWidth + 'x' + img.naturalHeight + ' ' + img.complete + ' ' + [d[0], d[1], d[2], d[3]].join(','));\
             }};\
             img.onerror = () => console.log('RESULT error');\
             console.log('BEFORE ' + img.complete);\
             img.src = '{url}';\
             </script></body>"
        ));
        page.settle();
        for _ in 0..50 {
            page.tick_web();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let log = page.web_console().join("\n");
        assert!(log.contains("RESULT 2x1 true 0,0,255,255"), "{log}");
    }

    #[test]
    fn an_image_that_will_not_decode_fires_error() {
        let mut page = page(
            "<body><script>\
             const img = new Image();\
             img.onload = () => console.log('RESULT load');\
             img.onerror = () => console.log('RESULT error ' + img.complete);\
             img.src = 'data:image/png;base64,AAAA';\
             </script></body>",
        );
        page.settle();
        for _ in 0..50 {
            page.tick_web();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let log = page.web_console().join("\n");
        assert!(log.contains("RESULT error true"), "{log}");
    }

    #[test]
    fn an_image_made_with_create_element_ns_is_a_real_image_element() {
        let mut page = page(
            "<body><script>\
             const a = document.createElementNS('http://www.w3.org/1999/xhtml', 'img');\
             const b = document.createElement('img');\
             console.log('RESULT ' + (a instanceof HTMLImageElement) + ' ' + (b instanceof HTMLImageElement) + ' ' + typeof a._startLoad);\
             </script></body>",
        );
        page.settle();
        let log = page.web_console().join("\n");
        assert!(log.contains("RESULT true true function"), "{log}");
    }

    #[test]
    fn a_response_body_larger_than_a_megabyte_reaches_the_script_whole() {
        // 3 MiB: base64 text of this is far past the old 1 MiB single-string limit.
        let body: &'static str = Box::leak("x".repeat(3 * 1024 * 1024).into_boxed_str());
        let origin = serve(vec![("/big", "application/octet-stream", body)]);
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| true));
        p.load_html_str(
            "<body><script>\
             fetch('/big').then((r) => r.arrayBuffer()).then((b) => console.log('RESULT ' + b.byteLength))\
               .catch((e) => console.log('RESULT error ' + e));\
             </script></body>",
            Some(format!("{origin}/start")),
        );
        wait_for(&mut p, |p| {
            p.web_console().iter().any(|l| l.contains("RESULT"))
        });
        let log = p.web_console().join("\n");
        assert!(log.contains(&format!("RESULT {}", body.len())), "{log}");
    }

    #[test]
    fn a_canvas_that_a_style_fades_in_with_a_finished_animation_is_shown() {
        let mut page = page(
            "<style>canvas { opacity: 0; animation: 1s ease-out .2s both canvas-fade-in }\
             @keyframes canvas-fade-in { from { opacity: 0 } to { opacity: 1 } }</style>\
             <body style='margin:0'><canvas id=c width=40 height=20></canvas><script>\
             const g = document.getElementById('c').getContext('2d');\
             g.fillStyle = '#00ff00'; g.fillRect(0, 0, 40, 20);\
             </script></body>",
        );
        page.settle();
        let pixmap = page.render_visible();
        let i = (10 * pixmap.width as usize + 20) * 4;
        assert_eq!(
            [pixmap.pixels[i], pixmap.pixels[i + 1], pixmap.pixels[i + 2]],
            [0, 255, 0],
            "the faded-in canvas is visible"
        );
    }

    #[test]
    fn large_binary_data_crosses_into_the_native_side_without_walking_it_in_script() {
        // 1 MiB through a WebGL buffer and a 2D canvas read-back. With the script-side
        // base64 (one character at a time, re-reading a long string each step) this took
        // minutes; natively it is milliseconds, so a generous bound separates them.
        let mut page = page(
            "<body><canvas id=c width=512 height=512></canvas><script>\
             const gl = document.getElementById('c').getContext('webgl');\
             const data = new Uint8Array(1 << 20); for (let i = 0; i < data.length; i++) data[i] = i & 255;\
             const b = gl.createBuffer(); gl.bindBuffer(gl.ARRAY_BUFFER, b);\
             const t0 = performance.now();\
             gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);\
             const pixels = new Uint8Array(512 * 512 * 4);\
             gl.readPixels(0, 0, 512, 512, gl.RGBA, gl.UNSIGNED_BYTE, pixels);\
             const g2 = document.createElement('canvas'); g2.width = 512; g2.height = 512;\
             const c2 = g2.getContext('2d'); const img = c2.getImageData(0, 0, 512, 512);\
             c2.putImageData(img, 0, 0);\
             console.log('RESULT ' + (performance.now() - t0 < 5000) + ' ' + g2.toDataURL().length);\
             </script></body>",
        );
        page.settle();
        let log = page.web_console().join("\n");
        assert!(log.contains("RESULT true "), "{log}");
    }

    #[test]
    fn scripts_are_off_unless_enabled() {
        let mut page = Page::new(400.0, 300.0);
        page.set_web_scripts(false);
        page.load_html_str("<body><p id=a>old</p><script>document.getElementById('a').textContent = 'new';</script></body>", None);
        assert_eq!(texts(&page), vec!["old"]);
    }

    #[test]
    fn a_reveal_class_added_by_script_changes_what_is_painted() {
        // The shape of a page that hides content until its script has run.
        let page = page(
            "<style>.js .hero { opacity: 0 } .js .shown { opacity: 1 }</style>\
             <body><h1 class=hero>Headline</h1>\
             <script>document.documentElement.classList.add('js');\
               new IntersectionObserver((es) => es.forEach(e => e.target.classList.add('shown'))).observe(document.querySelector('.hero'));</script></body>",
        );
        let alpha = |p: &Page| {
            p.render().commands.iter().find_map(|c| match c {
                PaintCommand::Text {
                    color: blueice_paint::Color::Rgba(_, _, _, a),
                    ..
                } => Some(*a),
                _ => None,
            })
        };
        // The heading is in the viewport, so the observer's first report (after
        // the scripts' layout) reveals it.
        let mut page = page;
        std::thread::sleep(Duration::from_millis(10));
        page.tick_web();
        assert_eq!(alpha(&page), Some(255));
    }

    #[test]
    fn domcontentloaded_and_load_fire_after_the_scripts() {
        let page = page(
            "<body><p id=p></p><script>const log = []; document.addEventListener('DOMContentLoaded', () => log.push('dcl:' + document.readyState));\
             window.addEventListener('load', () => { log.push('load:' + document.readyState); document.getElementById('p').textContent = log.join(','); });\
             log.push('script:' + document.readyState);</script></body>",
        );
        assert_eq!(
            texts(&page),
            vec!["script:loading,dcl:interactive,load:complete"]
        );
    }

    #[test]
    fn a_script_without_its_text_does_not_run_and_the_page_loads_anyway() {
        let page =
            page("<body><p>kept</p><script src='https://example.com/missing.js'></script></body>");
        assert_eq!(texts(&page), vec!["kept"]);
    }

    #[test]
    fn external_scripts_run_from_the_fetched_set_and_defer_runs_last() {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.pending_linked_scripts.insert(
            "a.js".into(),
            "window.order = (window.order || '') + 'a';".into(),
        );
        p.pending_linked_scripts.insert(
            "d.js".into(),
            "window.order = (window.order || '') + 'd';".into(),
        );
        p.load_html_str(
            "<body><p id=o></p><script defer src='d.js'></script><script src='a.js'></script>\
             <script>window.order = (window.order || '') + 'i'; </script>\
             <script defer>document.getElementById('o').textContent = window.order;</script></body>",
            Some("https://example.com/".into()),
        );
        // An inline `defer` is ignored, so the script after it saw only `ai`...
        assert_eq!(texts(&p), vec!["ai"]);
        // ...and the deferred external script ran last.
        p.web_turn(Turn::Probe(
            "document.getElementById('o').textContent = window.order;".to_string(),
        ));
        p.settle();
        assert_eq!(texts(&p), vec!["aid"]);
    }

    #[test]
    fn a_navigation_a_script_asks_for_is_handed_to_the_session() {
        let mut page = page("<body><script>location.assign('/next');</script></body>");
        assert_eq!(
            page.take_web_navigation().as_deref(),
            Some("https://example.com/next")
        );
        assert_eq!(page.take_web_navigation(), None);
    }

    #[test]
    fn pushstate_updates_the_pages_url_without_a_load() {
        let mut page = page("<body><script>history.pushState(null, '', '/pjax');</script></body>");
        assert_eq!(page.url(), Some("https://example.com/pjax"));
        assert_eq!(page.take_web_navigation(), None);
    }

    #[test]
    fn a_script_inserting_a_script_runs_it_inline_or_asks_for_it() {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| false));
        p.load_html_str(
            "<body><p id=p>x</p><script>\
               const s = document.createElement('script'); s.textContent = \"document.getElementById('p').textContent = 'dyn'\"; document.body.appendChild(s);\
               const e = document.createElement('script'); e.src = '/refused.js'; e.onerror = () => { document.getElementById('p').textContent += '+err'; }; document.body.appendChild(e);\
             </script></body>",
            Some("https://example.com/".into()),
        );
        assert_eq!(texts(&p), vec!["dyn+err"]);
    }

    #[test]
    fn module_scripts_import_the_fetched_graph() {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.pending_linked_modules.insert(
            "/lib/util.js".into(),
            "import { base } from './base.js'; export const greet = (n) => base + n;".into(),
        );
        p.pending_linked_modules
            .insert("/lib/base.js".into(), "export const base = 'hi ';".into());
        p.load_html_str(
            "<body><p id=p>x</p><script type=module>import { greet } from '/lib/util.js'; document.getElementById('p').textContent = greet('mod') + ':' + document.readyState;</script></body>",
            Some("https://example.com/dir/page".into()),
        );
        assert_eq!(texts(&p), vec!["hi", "mod:loading"]);
    }

    #[test]
    fn modules_run_after_classic_scripts_and_external_modules_resolve_by_key() {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.pending_linked_modules.insert(
            "/_a/m.js".into(),
            "window.order = (window.order || '') + 'm';".into(),
        );
        p.load_html_str(
            "<body><p id=p></p><script type=module src='/_a/m.js'></script><script>window.order = (window.order || '') + 'c';</script>\
             <script type=module>document.getElementById('p').textContent = window.order;</script></body>",
            Some("https://example.com/".into()),
        );
        assert_eq!(texts(&p), vec!["cm"]);
    }

    #[test]
    fn dynamic_import_from_a_classic_script_loads_a_fetched_module() {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.pending_linked_modules
            .insert("/x/chunk.js".into(), "export default 'chunk';".into());
        p.load_html_str(
            "<body><p id=p>x</p><script>import('/x/chunk.js').then((m) => { document.getElementById('p').textContent = m.default; });</script></body>",
            Some("https://example.com/".into()),
        );
        assert_eq!(texts(&p), vec!["chunk"]);
    }

    #[test]
    fn a_module_that_fails_to_link_ends_only_itself() {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.load_html_str(
            "<body><p id=p>kept</p><script type=module>import { nope } from '/missing.js';</script>\
             <script type=module>document.getElementById('p').textContent = 'second ran';</script></body>",
            Some("https://example.com/".into()),
        );
        assert_eq!(texts(&p), vec!["second", "ran"]);
    }

    #[test]
    fn content_below_the_fold_is_revealed_by_scrolling_to_it() {
        let mut page = page(
            "<style>.js .card { opacity: 0 } .js .card.in { opacity: 1 } .spacer { height: 1500px }</style>\
             <body><div class=spacer></div><p class=card>Later</p>\
             <script>document.documentElement.classList.add('js'); window.seen = [];\
               new IntersectionObserver((es) => es.forEach(e => { window.seen.push(e.isIntersecting); if (e.isIntersecting) e.target.classList.add('in'); })).observe(document.querySelector('.card'));\
               window.addEventListener('scroll', () => window.scrolled = window.scrollY, { passive: true });</script></body>",
        );
        let alpha = |p: &Page| {
            p.render().commands.iter().find_map(|c| match c {
                PaintCommand::Text {
                    color: blueice_paint::Color::Rgba(_, _, _, a),
                    ..
                } => Some(*a),
                _ => None,
            })
        };
        std::thread::sleep(Duration::from_millis(10));
        page.tick_web();
        assert_eq!(alpha(&page), Some(0), "off screen: still hidden");
        page.scroll_by(1400.0);
        page.tick_web();
        page.settle();
        assert_eq!(alpha(&page), Some(255), "scrolled into view: revealed");
        let mut seen = String::new();
        page.web_eval("console.log(window.scrolled + ' ' + window.seen.join(','))");
        for line in page.web_console() {
            seen = line;
        }
        assert_eq!(
            seen, "[log] 1259.2 false,true",
            "the scroll offset is clamped to the page"
        );
    }

    /// Serves `routes` (path -> content type, body) on a local port.
    fn serve(routes: Vec<(&'static str, &'static str, &'static str)>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                // The head, then as much body as Content-Length promises.
                stream
                    .set_read_timeout(Some(Duration::from_millis(300)))
                    .unwrap();
                let mut received = Vec::new();
                let mut buf = [0u8; 4096];
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    received.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&received).to_string();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let wanted: usize = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if body.len() >= wanted {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&received).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                let method = request.split_whitespace().next().unwrap_or("").to_string();
                let body_in = request.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                let reply = routes
                    .iter()
                    .find(|(p, _, _)| *p == path)
                    .map(|(_, kind, body)| {
                        let body = body.replace("{method}", &method).replace("{body}", &body_in);
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    })
                    .unwrap_or_else(|| "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        origin
    }

    /// Ticks until `done` or three seconds pass.
    fn wait_for(page: &mut Page, done: impl Fn(&Page) -> bool) {
        let until = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < until && !done(page) {
            page.tick_web();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn fetch_json_and_render_a_list_through_inner_html() {
        let origin = serve(vec![(
            "/items.json",
            "application/json",
            "[{\"n\":\"alpha\"},{\"n\":\"beta\"}]",
        )]);
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| true));
        p.load_html_str(
            "<body><ul id=l></ul><script>\
               fetch('/items.json').then(r => { if (!r.ok) throw new Error('bad'); return r.json(); })\
                 .then(items => { document.getElementById('l').innerHTML = items.map(i => '<li>' + i.n + '</li>').join(''); });\
             </script></body>",
            Some(format!("{origin}/")),
        );
        wait_for(&mut p, |p| texts(p).contains(&"beta".to_string()));
        assert!(
            texts(&p).contains(&"alpha".to_string()) && texts(&p).contains(&"beta".to_string()),
            "{:?}",
            texts(&p)
        );
    }

    #[test]
    fn xmlhttprequest_posts_a_body_and_reads_the_response() {
        let origin = serve(vec![("/echo", "text/plain", "{method}:{body}")]);
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| true));
        p.load_html_str(
            "<body><p id=o>wait</p><script>\
               const x = new XMLHttpRequest(); x.open('POST', '/echo'); x.setRequestHeader('Content-Type', 'text/plain');\
               x.onload = () => { document.getElementById('o').textContent = x.status + ' ' + x.responseText + ' ' + x.getResponseHeader('content-type'); };\
               x.send('payload');\
             </script></body>",
            Some(format!("{origin}/")),
        );
        wait_for(&mut p, |p| texts(p).iter().any(|t| t.contains("payload")));
        assert!(
            texts(&p).join(" ").contains("200 POST:payload text/plain"),
            "{:?}",
            texts(&p)
        );
    }

    #[test]
    fn a_blob_body_is_sent_as_its_bytes_through_fetch_and_xmlhttprequest() {
        let origin = serve(vec![("/echo", "text/plain", "{method}:{body}")]);
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| true));
        p.load_html_str(
            "<body><p id=a>wait</p><p id=b>wait</p><script>\
               fetch('/echo', { method: 'POST', body: new Blob(['blob-one'], { type: 'text/plain' }) })\
                 .then(r => r.text()).then(t => { document.getElementById('a').textContent = 'fetch ' + t; });\
               const x = new XMLHttpRequest(); x.open('POST', '/echo');\
               x.onload = () => { document.getElementById('b').textContent = 'xhr ' + x.responseText; };\
               x.send(new Blob(['blob-two'], { type: 'text/plain' }));\
             </script></body>",
            Some(format!("{origin}/")),
        );
        wait_for(&mut p, |p| {
            let t = texts(p).join(" ");
            t.contains("blob-one") && t.contains("blob-two")
        });
        let all = texts(&p).join(" ");
        assert!(all.contains("fetch POST:blob-one"), "{all}");
        assert!(all.contains("xhr POST:blob-two"), "{all}");
        assert!(!all.contains("[object Blob]"), "{all}");
    }

    #[test]
    fn attribute_nodes_can_be_read_set_and_removed() {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.load_html_str(
            "<body><p id=o class=k data-x=1>x</p><script>\
               const el = document.getElementById('o');\
               const log = [];\
               const cls = el.getAttributeNode('class');\
               log.push(cls.name + '=' + cls.value + ' owner:' + (cls.ownerElement === el));\
               log.push('missing:' + el.getAttributeNode('nope'));\
               const made = document.createAttribute('title'); made.value = 'hello';\
               log.push('replaced:' + el.setAttributeNode(made));\
               log.push('set:' + el.getAttribute('title') + ' owner:' + (made.ownerElement === el));\
               const again = document.createAttribute('title'); again.value = 'bye';\
               log.push('previous:' + el.setAttributeNode(again).value);\
               log.push('removed:' + el.removeAttributeNode(el.getAttributeNode('data-x')).name);\
               log.push('gone:' + el.hasAttribute('data-x'));\
               try { el.removeAttributeNode(made); } catch (e) { log.push('stale:' + e.name); }\
               console.log('RESULT ' + log.join(' | '));\
             </script></body>",
            Some("http://page.example/".to_string()),
        );
        let line = p
            .web_console()
            .iter()
            .find(|l| l.contains("RESULT"))
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            line,
            "[log] RESULT class=k owner:true | missing:null | replaced:null | set:hello owner:true | previous:hello | removed:data-x | gone:false | stale:NotFoundError"
        );
    }

    #[test]
    fn attributes_taken_from_the_attribute_list_can_be_removed_one_by_one() {
        // What React's hydration does to the extra attributes of an element.
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.load_html_str(
            "<body><p id=o class=k data-x=1 title=t>x</p><script>\
               const el = document.getElementById('o');\
               const same = el.attributes[0] === el.getAttributeNode(el.attributes[0].name);\
               const names = [];\
               for (const a of Array.from(el.attributes)) { if (a.name !== 'id') { el.removeAttributeNode(a); names.push(a.name); } }\
               console.log('RESULT ' + same + ' ' + names.join('+') + ' left:' + el.getAttributeNames().join('+'));\
             </script></body>",
            Some("http://page.example/".to_string()),
        );
        let line = p
            .web_console()
            .iter()
            .find(|l| l.contains("RESULT"))
            .cloned()
            .unwrap_or_default();
        assert_eq!(line, "[log] RESULT true class+data-x+title left:id");
    }

    #[test]
    fn the_attribute_list_is_live() {
        // React clears an element with `for (l = el.attributes; l.length;) el.removeAttributeNode(l[0])`.
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.load_html_str(
            "<body><p id=o class=k data-x=1>x</p><script>\
               const el = document.getElementById('o');\
               const list = el.attributes;\
               const log = ['len:' + list.length, 'first:' + list[0].name, 'item:' + list.item(1).name, 'named:' + list.getNamedItem('data-x').value, 'out:' + list[9]];\
               el.setAttribute('title', 't');\
               log.push('grew:' + list.length);\
               let guard = 0;\
               for (const l = el.attributes; l.length && guard++ < 10;) el.removeAttributeNode(l[0]);\
               log.push('emptied:' + list.length + ' ' + el.hasAttributes());\
               el.setAttribute('a', '1'); el.setAttribute('b', '2');\
               log.push('keys:' + Object.keys(list).join(',') + ' spread:' + [...list].map(a => a.name).join('+') + ' map:' + Array.prototype.map.call(list, a => a.value).join('+'));\
               log.push('removeNamed:' + list.removeNamedItem('a').name + ' left:' + list.length);\
               console.log('RESULT ' + log.join(' | '));\
             </script></body>",
            Some("http://page.example/".to_string()),
        );
        let line = p
            .web_console()
            .iter()
            .find(|l| l.contains("RESULT"))
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            line,
            "[log] RESULT len:3 | first:id | item:class | named:1 | out:undefined | grew:4 | emptied:0 false | keys:0,1 spread:a+b map:1+2 | removeNamed:a left:1"
        );
    }

    #[test]
    fn node_lists_and_html_collections_have_item_and_named_item() {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.load_html_str(
            "<body><p id=a class=c name=n1>1</p><p id=b class=c>2</p><script>\
               const log = [];\
               log.push(document.querySelectorAll('p').item(1).id);\
               log.push(document.getElementsByTagName('p').item(0).id);\
               log.push(document.getElementsByClassName('c').item(1).id);\
               log.push(document.body.children.item(0).id);\
               log.push(String(document.body.childNodes.item(99)));\
               log.push(String(document.querySelectorAll('p').item(-1)));\
               log.push(document.body.children.namedItem('b').id);\
               log.push(String(document.body.children.namedItem('zzz')));\
               log.push(String(document.getElementsByClassName('nothing').item(0)));\
               log.push(document.body.children.length + ':' + Array.from(document.body.children).length);\
               console.log('RESULT ' + log.join(','));\
             </script></body>",
            Some("http://page.example/".to_string()),
        );
        let line = p
            .web_console()
            .iter()
            .find(|l| l.contains("RESULT"))
            .cloned()
            .unwrap_or_default();
        assert_eq!(line, "[log] RESULT b,a,b,a,null,null,b,null,null,3:3");
    }

    #[test]
    fn a_burst_of_requests_is_queued_not_refused() {
        // A page may start far more requests in one turn than the engine runs at once; the
        // rest wait their turn instead of throwing (which took React's whole tree down).
        let origin = serve(vec![("/n", "text/plain", "ok")]);
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| true));
        p.load_html_str(
            "<body><script>\
               let ok = 0, bad = 0;\
               for (let i = 0; i < 150; i++) fetch('/n?i=' + i).then(r => r.text()).then(() => { ok++; report(); }, () => { bad++; report(); });\
               function report() { if (ok + bad === 150) console.log('RESULT ' + ok + ' ok ' + bad + ' failed'); }\
             </script></body>",
            Some(format!("{origin}/")),
        );
        let until = std::time::Instant::now() + Duration::from_secs(20);
        while std::time::Instant::now() < until
            && !p.web_console().iter().any(|l| l.contains("RESULT"))
        {
            p.tick_web();
            std::thread::sleep(Duration::from_millis(5));
        }
        let line = p
            .web_console()
            .iter()
            .find(|l| l.contains("RESULT"))
            .cloned()
            .unwrap_or_default();
        assert_eq!(line, "[log] RESULT 150 ok 0 failed");
    }

    #[test]
    fn a_pjax_navigation_fetches_a_fragment_swaps_it_in_and_pushes_the_url() {
        let origin = serve(vec![("/next", "text/html", "<html><head><title>Next</title></head><body><main id=m>second page</main></body></html>")]);
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| true));
        p.load_html_str(
            "<body><a id=go href='/next'>go</a><main id=m>first page</main><script>\
               document.addEventListener('click', (e) => {\
                 const a = e.target.closest('a'); if (!a) return; e.preventDefault();\
                 fetch(a.href).then(r => r.text()).then(html => {\
                   const doc = new DOMParser().parseFromString(html, 'text/html');\
                   document.getElementById('m').replaceWith(doc.getElementById('m'));\
                   document.title = doc.title; history.pushState({ pjax: 1 }, '', a.getAttribute('href'));\
                 });\
               });\
             </script></body>",
            Some(format!("{origin}/start")),
        );
        assert_eq!(p.take_web_navigation(), None);
        p.web_eval("__blueice.dispatch(document.getElementById('go')._id, 'click')");
        wait_for(&mut p, |p| texts(p).iter().any(|t| t == "second"));
        assert!(texts(&p).contains(&"second".to_string()), "{:?}", texts(&p));
        assert_eq!(p.url(), Some(format!("{origin}/next").as_str()));
        assert_eq!(p.take_web_navigation(), None, "no page load happened");
    }

    #[test]
    fn a_blocked_request_rejects_and_a_page_without_review_cannot_fetch() {
        let origin = serve(vec![("/x", "text/plain", "x")]);
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| false));
        p.load_html_str(
            "<body><p id=o>pending</p><script>fetch('/x').then(() => document.getElementById('o').textContent = 'allowed', () => document.getElementById('o').textContent = 'blocked');</script></body>",
            Some(format!("{origin}/")),
        );
        wait_for(&mut p, |p| texts(p).contains(&"blocked".to_string()));
        assert_eq!(texts(&p), vec!["blocked"]);
        let mut q = Page::new(400.0, 300.0);
        q.set_web_scripts(true);
        q.load_html_str(
            "<body><p id=o>pending</p><script>fetch('/x').catch(() => document.getElementById('o').textContent = 'refused');</script></body>",
            Some(format!("{origin}/")),
        );
        wait_for(&mut q, |p| texts(p).contains(&"refused".to_string()));
        assert_eq!(texts(&q), vec!["refused"]);
    }

    #[test]
    fn aborting_a_fetch_rejects_with_an_abort_error() {
        let origin = serve(vec![("/x", "text/plain", "x")]);
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| true));
        p.load_html_str(
            "<body><p id=o>pending</p><script>const c = new AbortController(); fetch('/x', { signal: c.signal }).catch(e => document.getElementById('o').textContent = e.name); c.abort();</script></body>",
            Some(format!("{origin}/")),
        );
        wait_for(&mut p, |p| texts(p).contains(&"AbortError".to_string()));
        assert_eq!(texts(&p), vec!["AbortError"]);
    }

    /// A server that answers every request `200 OK` with no CORS headers and reports the first
    /// line (method and path) of each request it gets.
    fn recording_server() -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                stream
                    .set_read_timeout(Some(Duration::from_millis(300)))
                    .unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let _ = tx.send(text.lines().next().unwrap_or("").to_string());
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
            }
        });
        (origin, rx)
    }

    fn cross_origin_page(script: &str) -> Page {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.set_network_review(Arc::new(|_| true));
        p.load_html_str(
            &format!("<body><p id=o>pending</p><script>{script}</script></body>"),
            Some("http://page.example/".to_string()),
        );
        p
    }

    fn outcome(p: &mut Page) -> String {
        wait_for(p, |p| {
            p.web_console().iter().any(|l| l.contains("RESULT"))
        });
        p.web_console()
            .iter()
            .find(|l| l.contains("RESULT"))
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn a_no_cors_fetch_to_another_origin_sends_and_resolves_an_opaque_response() {
        let (origin, rx) = recording_server();
        let mut p = cross_origin_page(&format!(
            "fetch('{origin}/track', {{ mode: 'no-cors' }}).then(r => console.log('RESULT ' + r.type + ' ' + r.status + ' ' + r.ok), e => console.log('RESULT rejected'));"
        ));
        assert_eq!(outcome(&mut p), "[log] RESULT opaque 0 false");
        assert_eq!(rx.try_recv().unwrap(), "GET /track HTTP/1.1");
    }

    #[test]
    fn the_same_fetch_in_cors_mode_is_refused() {
        let (origin, _rx) = recording_server();
        let mut p = cross_origin_page(&format!(
            "fetch('{origin}/track').then(r => console.log('RESULT resolved'), e => console.log('RESULT rejected'));"
        ));
        assert_eq!(outcome(&mut p), "[log] RESULT rejected");
    }

    #[test]
    fn a_beacon_with_a_plain_body_reaches_another_origin_without_preflight() {
        let (origin, rx) = recording_server();
        let mut p = cross_origin_page(&format!(
            "console.log('RESULT ' + navigator.sendBeacon('{origin}/beacon', 'ping=1'));"
        ));
        assert_eq!(outcome(&mut p), "[log] RESULT true");
        let first = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(first, "POST /beacon HTTP/1.1", "no OPTIONS before it");
    }

    #[test]
    fn a_beacon_with_a_json_body_is_a_cors_request_and_is_preflighted() {
        let (origin, rx) = recording_server();
        let mut p = cross_origin_page(&format!(
            "console.log('RESULT ' + navigator.sendBeacon('{origin}/beacon', new Blob(['{{}}'], {{ type: 'application/json' }})));"
        ));
        assert_eq!(outcome(&mut p), "[log] RESULT true");
        let first = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(first, "OPTIONS /beacon HTTP/1.1");
        // The server grants nothing, so the POST itself is never sent.
        assert!(rx.recv_timeout(Duration::from_millis(500)).is_err());
    }
}
