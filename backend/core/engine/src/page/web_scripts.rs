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
    dom_bridge::{DomState, NavigationRequest},
    WebHost,
};
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Decides whether a script's own request for one absolute URL may be made.
pub(crate) type NetworkReview = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// A page's in-flight requests: each runs on its own thread and reports here.
pub(crate) struct WebNet {
    tx: std::sync::mpsc::Sender<(u64, crate::web::net::NetOutcome)>,
    rx: std::sync::mpsc::Receiver<(u64, crate::web::net::NetOutcome)>,
    inflight: usize,
}

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
                if (module || CLASSIC_TYPES.contains(&kind.as_str())) && attr("nomodule").is_none() {
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
                        deferred: src.is_some() && attr("defer").is_some() && attr("async").is_none(),
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
    /// every request is refused.
    pub(crate) fn set_network_review(&mut self, review: NetworkReview) {
        self.web_review = Some(review);
    }

    pub(crate) fn take_web_navigation(&mut self) -> Option<String> {
        self.web_navigation.take()
    }

    /// Runs `f` on the realm with the page's DOM, styles and layout boxes
    /// handed over, then takes them back; a changed DOM is restyled and laid
    /// out and marked for publication. Returns the external scripts the turn
    /// asked to load.
    fn web_turn_once(&mut self, f: &mut dyn FnMut(&mut WebHost)) -> (Vec<(NodeId, String)>, bool) {
        let Some(mut host) = self.web.take() else {
            return (Vec::new(), false);
        };
        let doc = std::mem::replace(&mut self.doc, Document::new());
        let styles = std::mem::take(&mut self.styles);
        let mut rects = HashMap::new();
        collect_rects(&self.fragment, 0.0, 0.0, &mut rects);
        let url = self.url.clone();
        let viewport = (self.viewport_width, self.viewport_height);
        host.with_state(|s| {
            s.doc = doc;
            s.styles = styles;
            s.rects = rects;
            s.viewport = viewport;
            if let Some(url) = url {
                s.url = url;
            }
            s.dirty = false;
        });
        f(&mut host);
        let (doc, styles, dirty, navigations, requests, url, jobs) = host.with_state(|s| {
            (
                std::mem::replace(&mut s.doc, Document::new()),
                std::mem::take(&mut s.styles),
                std::mem::take(&mut s.dirty),
                std::mem::take(&mut s.navigations),
                std::mem::take(&mut s.script_requests),
                s.url.clone(),
                std::mem::take(&mut s.net_jobs),
            )
        });
        self.doc = doc;
        self.styles = styles;
        self.web = Some(host);
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
        if dirty {
            self.restyle_and_relayout();
            self.script_change_unpublished = true;
            self.script_change_unpublished_by_web = true;
        }
        (requests, dirty)
    }

    /// One turn, then the scripts it inserted: each fetched under the same
    /// review as any request and run (or failed) as its own turn. A bounded
    /// number of rounds, so scripts that insert scripts cannot run forever.
    fn web_turn(&mut self, mut f: impl FnMut(&mut WebHost)) {
        let (mut pending, mut changed) = self.web_turn_once(&mut f);
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
                let ok = source.is_some();
                let call = format!(
                    "__blueice.scriptLoaded({}, {}, {});",
                    node.as_u64(),
                    js_string(&source.unwrap_or_default()),
                    ok
                );
                let (more, dirty) = self.web_turn_once(&mut |host: &mut WebHost| {
                    let _ = host.run("blueice:script-load", &call);
                });
                changed |= dirty;
                next.extend(more);
            }
            pending = next;
        }
        // A change of the document moves things, which observers of visibility
        // report on once layout has caught up; their callbacks may change it
        // again, a few rounds at most.
        for _ in 0..3 {
            if !changed {
                break;
            }
            let (_, dirty) = self.web_turn_once(&mut |host: &mut WebHost| {
                let _ = host.run("blueice:layout", "__blueice.afterLayout();");
            });
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
            }
        });
        for job in jobs {
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
            self.web_turn(move |host| {
                let _ = host.run("blueice:net", &call);
            });
        }
    }

    /// Starts this document's scripts, if they are enabled: the realm, every
    /// classic script, then `DOMContentLoaded` and `load`.
    pub(super) fn start_web_scripts(&mut self) {
        self.web = None;
        if !self.web_scripts_on() {
            return;
        }
        let mut scripts = collect_scripts(&self.doc);
        // The realm is built against (a copy of) the real document: its root
        // node's id is part of what the platform captures.
        let state = DomState::new(
            self.doc.clone(),
            self.url.as_deref().unwrap_or("about:blank"),
            (self.viewport_width, self.viewport_height),
        );
        let host = match WebHost::new(state) {
            Ok(host) => host,
            Err(error) => {
                if std::env::var("BI_DIAG").is_ok() {
                    eprintln!("page scripts could not start: {error}");
                }
                return;
            }
        };
        self.web = Some(host);
        let linked = self.linked_scripts.clone();
        let modules = self.linked_modules.clone();
        let document = self.url.as_deref().and_then(|u| url::Url::parse(u).ok());
        self.web_turn(|host| {
            let document = document.clone();
            let document_dir = document
                .as_ref()
                .map(|u| {
                    let path = u.path();
                    path.rsplit_once('/').map_or("", |(dir, _)| dir).to_string()
                })
                .unwrap_or_default();
            host.set_module_sources(
                &document
                    .as_ref()
                    .map(|u| crate::scripts_fetch::module_key(u, u))
                    .unwrap_or_default(),
                modules.clone(),
            );
            // Parser-blocking and async classic scripts in order, then the
            // deferred ones, then modules (which are deferred by nature).
            let all = std::mem::take(&mut scripts);
            let (module_scripts, classic): (Vec<_>, Vec<_>) = all.into_iter().partition(|s| s.module);
            let (deferred, immediate): (Vec<_>, Vec<_>) = classic.into_iter().partition(|s| s.deferred);
            for script in immediate.into_iter().chain(deferred) {
                let (name, source) = match &script.src {
                    Some(src) => match linked.get(src) {
                        Some(text) => (src.clone(), text.clone()),
                        None => continue,
                    },
                    None => ("inline script".to_string(), script.inline),
                };
                let id = script.node.as_u64();
                let _ = host.run("blueice:current", &format!("__blueice.setCurrentScript({id});"));
                let _ = host.run(&name, &source);
            }
            for (n, script) in module_scripts.into_iter().enumerate() {
                let key_source = match &script.src {
                    Some(src) => document
                        .as_ref()
                        .and_then(|d| d.join(src).ok().map(|u| (crate::scripts_fetch::module_key(&u, d), src.clone())))
                        .and_then(|(key, _)| modules.get(&key).map(|text| (key, text.clone()))),
                    None => Some((
                        format!("{document_dir}/__inline_module_{n}.js"),
                        script.inline.clone(),
                    )),
                };
                if let Some((key, text)) = key_source {
                    let id = script.node.as_u64();
                    let _ = host.run("blueice:current", &format!("__blueice.setCurrentScript({id});"));
                    let _ = host.run_module(&key, &text, &modules);
                }
            }
            let _ = host.run(
                "blueice:ready",
                "__blueice.setCurrentScript(null); __blueice.setReadyState('interactive'); __blueice.fireDocument('DOMContentLoaded', { bubbles: true });",
            );
            let _ = host.run(
                "blueice:load",
                "__blueice.setReadyState('complete'); for (const i of document.querySelectorAll('img')) i.dispatchEvent(new Event('load')); __blueice.fire('load'); __blueice.fire('pageshow');",
            );
        });
    }

    /// Runs the timers that are due. True when the document changed.
    pub(crate) fn tick_web(&mut self) -> bool {
        // Scrolling reaches the page's scripts as a scroll event and a fresh
        // look at what is visible.
        if self.web.is_some() && (self.scroll_y - self.web_scroll_seen).abs() > 0.5 {
            self.web_scroll_seen = self.scroll_y;
            let y = self.scroll_y;
            self.web_turn(|host| {
                let _ = host.run("blueice:scroll", &format!("__blueice.scrolled({y});"));
            });
        }
        self.deliver_net_results();
        let due = self
            .web
            .as_ref()
            .and_then(|host| host.next_timer_due().map(|due| due <= host.now_ms()))
            .unwrap_or(false);
        if !due {
            return false;
        }
        self.web_turn(|host| {
            host.tick();
        });
        std::mem::take(&mut self.script_change_unpublished_by_web)
    }

    /// How long until this document's scripts next have work, if ever.
    pub(crate) fn web_wake_in(&self) -> Option<Duration> {
        let host = self.web.as_ref()?;
        // While requests are out, look for their answers often.
        let waiting = self
            .web_net
            .as_ref()
            .is_some_and(|net| net.inflight > 0)
            .then_some(Duration::from_millis(10));
        let timer = host
            .next_timer_due()
            .map(|due| Duration::from_millis(due.saturating_sub(host.now_ms())));
        match (waiting, timer) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Runs `source` in the page's realm as a diagnostic probe.
    #[allow(dead_code)]
    pub(crate) fn web_eval(&mut self, source: &str) {
        let source = source.to_string();
        self.web_turn(move |host| {
            let _ = host.run("probe", &source);
        });
    }

    /// What the realm's scripts have logged (diagnostics).
    #[allow(dead_code)]
    pub(crate) fn web_console(&self) -> Vec<String> {
        self.web
            .as_ref()
            .map(|host| host.console().to_vec())
            .unwrap_or_default()
    }
}

fn js_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
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

    #[test]
    fn inline_scripts_run_in_order_and_their_changes_are_painted() {
        let page = page(
            "<body><p id=a>old</p><script>document.getElementById('a').textContent = 'one';</script>\
             <script>document.getElementById('a').textContent += ' two';</script></body>",
        );
        assert_eq!(texts(&page), vec!["one", "two"]);
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
            p.render()
                .commands
                .iter()
                .find_map(|c| match c {
                    PaintCommand::Text { color: blueice_paint::Color::Rgba(_, _, _, a), .. } => Some(*a),
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
        assert_eq!(texts(&page), vec!["script:loading,dcl:interactive,load:complete"]);
    }

    #[test]
    fn a_script_without_its_text_does_not_run_and_the_page_loads_anyway() {
        let page = page("<body><p>kept</p><script src='https://example.com/missing.js'></script></body>");
        assert_eq!(texts(&page), vec!["kept"]);
    }

    #[test]
    fn external_scripts_run_from_the_fetched_set_and_defer_runs_last() {
        let mut p = Page::new(400.0, 300.0);
        p.set_web_scripts(true);
        p.pending_linked_scripts.insert("a.js".into(), "window.order = (window.order || '') + 'a';".into());
        p.pending_linked_scripts.insert("d.js".into(), "window.order = (window.order || '') + 'd';".into());
        p.load_html_str(
            "<body><p id=o></p><script defer src='d.js'></script><script src='a.js'></script>\
             <script>window.order = (window.order || '') + 'i'; </script>\
             <script defer>document.getElementById('o').textContent = window.order;</script></body>",
            Some("https://example.com/".into()),
        );
        // An inline `defer` is ignored, so the script after it saw only `ai`...
        assert_eq!(texts(&p), vec!["ai"]);
        // ...and the deferred external script ran last.
        p.web_turn(|host| {
            let _ = host.run("t", "document.getElementById('o').textContent = window.order;");
        });
        assert_eq!(texts(&p), vec!["aid"]);
    }

    #[test]
    fn a_navigation_a_script_asks_for_is_handed_to_the_session() {
        let mut page = page("<body><script>location.assign('/next');</script></body>");
        assert_eq!(page.take_web_navigation().as_deref(), Some("https://example.com/next"));
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
        p.pending_linked_modules.insert("/lib/util.js".into(), "import { base } from './base.js'; export const greet = (n) => base + n;".into());
        p.pending_linked_modules.insert("/lib/base.js".into(), "export const base = 'hi ';".into());
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
        p.pending_linked_modules.insert("/_a/m.js".into(), "window.order = (window.order || '') + 'm';".into());
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
        p.pending_linked_modules.insert("/x/chunk.js".into(), "export default 'chunk';".into());
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
                PaintCommand::Text { color: blueice_paint::Color::Rgba(_, _, _, a), .. } => Some(*a),
                _ => None,
            })
        };
        std::thread::sleep(Duration::from_millis(10));
        page.tick_web();
        assert_eq!(alpha(&page), Some(0), "off screen: still hidden");
        page.scroll_by(1400.0);
        page.tick_web();
        assert_eq!(alpha(&page), Some(255), "scrolled into view: revealed");
        let mut seen = String::new();
        page.web_eval("console.log(window.scrolled + ' ' + window.seen.join(','))");
        for line in page.web_console() {
            seen = line;
        }
        assert_eq!(seen, "[log] 1259.2 false,true", "the scroll offset is clamped to the page");
    }

    /// Serves `routes` (path -> content type, body) on a local port.
    fn serve(routes: Vec<(&'static str, &'static str, &'static str)>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
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
        let origin = serve(vec![("/items.json", "application/json", "[{\"n\":\"alpha\"},{\"n\":\"beta\"}]")]);
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
        assert!(texts(&p).contains(&"alpha".to_string()) && texts(&p).contains(&"beta".to_string()), "{:?}", texts(&p));
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
        assert!(texts(&p).join(" ").contains("200 POST:payload text/plain"), "{:?}", texts(&p));
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
}
