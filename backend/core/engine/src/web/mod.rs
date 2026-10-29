// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Page scripts: ordinary web JavaScript run against the page's own DOM.
//!
//! One [`WebHost`] owns one BlueJS realm for one document. The realm gets a
//! single native function (`__bi`, see [`dom_bridge`]) and the JavaScript
//! layer in `bootstrap.js`, which builds the DOM, events, `window`, `location`,
//! `history`, storage and the rest of the platform on top of it. Nothing runs
//! unless the embedder constructs a host and hands it scripts: the default
//! session does not.
//!
//! The VM is memory-, fuel- and string-bounded by BlueJS itself and runs on the
//! thread that calls it, so a script that loops or allocates degrades its own
//! document and nothing else. Network access is not part of this module: a
//! request a script makes is recorded for the embedder, which reviews it.

pub(crate) mod dom_bridge;
pub(crate) mod net;

use blueice_bluejs::{
    compile, compile_module, parse, parse_module, Bytecode, HostFunctionError, HostValue, Vm,
    VmConfig,
};
use std::collections::HashMap;
use dom_bridge::{DomState, Reply};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// Per-execution fuel for page code: a bundle's top level can run for a while.
const INSTRUCTION_BUDGET: u64 = 400_000_000;
/// Heap a document's script may hold.
const MAX_HEAP_BYTES: usize = 384 * 1024 * 1024;
/// Console lines kept for diagnostics.
const MAX_CONSOLE_LINES: usize = 500;

const BOOTSTRAP: &str = include_str!("bootstrap.js");

pub(crate) struct WebHost {
    vm: Vm,
    state: Rc<RefCell<DomState>>,
    started: Instant,
    console: Vec<String>,
    /// Every module of the page, compiled; passed whole to each module run so
    /// that all of them share one registry and one set of instances.
    modules: HashMap<String, Bytecode>,
}

// SAFETY: a `WebHost` owns its whole realm. Every `Rc` in it -- the VM's own
// bookkeeping and the DOM state shared with the VM's host function -- is reachable
// only through the `WebHost` (the host function is stored inside the VM, and
// nothing else holds a clone), so moving the host to another thread moves all
// of those references together; it is never used from two threads at once.
unsafe impl Send for WebHost {}

fn reply_to_host(reply: Reply) -> HostValue {
    match reply {
        Reply::Undefined => HostValue::Undefined,
        Reply::Null => HostValue::Null,
        Reply::Bool(b) => HostValue::Bool(b),
        Reply::Number(n) => HostValue::Number(n),
        Reply::Text(s) => HostValue::String(s.into()),
    }
}

fn host_to_reply(value: &HostValue) -> Reply {
    match value {
        HostValue::Undefined => Reply::Undefined,
        HostValue::Null => Reply::Null,
        HostValue::Bool(b) => Reply::Bool(*b),
        HostValue::Number(n) => Reply::Number(*n),
        HostValue::String(s) => Reply::Text(String::from_utf16_lossy(s.as_code_units())),
    }
}

impl WebHost {
    /// A realm for the document in `state`, with the platform installed.
    pub fn new(state: DomState) -> Result<Self, String> {
        let mut config = VmConfig::default();
        config.instruction_budget = INSTRUCTION_BUDGET;
        config.heap.max_heap_bytes = MAX_HEAP_BYTES;
        let mut vm = Vm::new(config).map_err(|e| format!("{e:?}"))?;
        let started = Instant::now();
        let state = Rc::new(RefCell::new(state));
        vm.install_web_file_api().map_err(|e| e.to_string())?;
        let clock_start = started;
        vm.install_host_timer_functions_with_clock(move || {
            clock_start.elapsed().as_millis() as u64
        })
        .map_err(|e| e.to_string())?;
        vm.install_host_console().map_err(|e| e.to_string())?;
        let _ = vm.install_host_error_reporting();
        let bridge_state = Rc::clone(&state);
        vm.install_host_function("__bi", 1, move |args: &[HostValue]| {
            let Some(HostValue::String(op)) = args.first() else {
                return Err(HostFunctionError::new("a DOM primitive needs a name"));
            };
            let op = String::from_utf16_lossy(op.as_code_units());
            let replies: Vec<Reply> = args[1..].iter().map(host_to_reply).collect();
            bridge_state
                .borrow_mut()
                .call(&op, &replies)
                .map(reply_to_host)
                .map_err(HostFunctionError::new)
        })
        .map_err(|e| e.to_string())?;
        let mut host = WebHost {
            vm,
            state,
            started,
            console: Vec::new(),
            modules: HashMap::new(),
        };
        host.run("blueice:bootstrap", BOOTSTRAP)
            .map_err(|e| format!("bootstrap failed: {e}"))?;
        Ok(host)
    }

    /// Milliseconds on the realm's clock.
    pub fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// Runs `source` as a classic script. A script that fails to parse or
    /// throws ends only itself; the reason is returned (and kept in the
    /// console).
    pub fn run(&mut self, name: &str, source: &str) -> Result<(), String> {
        let program = parse(source).map_err(|e| format!("{name}: {e:?}"))?;
        let code = compile(&program).map_err(|e| format!("{name}: {e:?}"))?;
        let result = self.vm.execute_script(&code);
        let _ = self.vm.run_host_microtask_checkpoint();
        self.collect_console();
        result.map(|_| ()).map_err(|e| {
            let message = format!("{name}: {e}");
            self.note(&message);
            message
        })
    }

    /// Makes `sources` (module key -> text) available to `import()`, with
    /// relative specifiers of classic scripts resolved against `referrer`.
    ///
    /// Every module is compiled up front: a dynamic import links the module
    /// it names against the registry the realm holds, so what that module
    /// itself imports has to be there already. A module that does not parse or
    /// compile is left out (importing it then fails, like an unfetched one).
    pub fn set_module_sources(&mut self, referrer: &str, sources: HashMap<String, String>) {
        let compiled: HashMap<String, Bytecode> = sources
            .iter()
            .filter_map(|(key, text)| {
                let module = parse_module(text).ok()?;
                Some((key.clone(), compile_module(&module).ok()?))
            })
            .collect();
        self.vm.set_module_loader_context(referrer, compiled.clone());
        self.vm.set_dynamic_module_sources(sources);
        self.modules = compiled;
    }

    /// Runs `source` as the module `key`, linking its static imports from
    /// `sources`. A module that cannot be parsed or linked ends only itself.
    pub fn run_module(
        &mut self,
        key: &str,
        source: &str,
        sources: &HashMap<String, String>,
    ) -> Result<(), String> {
        // The entry (an inline module, say) joins the page's modules, and so
        // does anything it imports that was not compiled with them.
        let mut added: Vec<String> = Vec::new();
        let mut stack = vec![(key.to_string(), source.to_string())];
        while let Some((name, text)) = stack.pop() {
            if self.modules.contains_key(&name) && name != key {
                continue;
            }
            let module = parse_module(&text).map_err(|e| format!("{name}: {e:?}"))?;
            for request in &module.requests {
                let target = crate::scripts_fetch::resolve_key(&name, &request.specifier);
                if !self.modules.contains_key(&target) {
                    if let Some(dependency) = sources.get(&target) {
                        stack.push((target, dependency.clone()));
                    }
                }
            }
            let code = compile_module(&module).map_err(|e| format!("{name}: {e:?}"))?;
            added.push(name.clone());
            self.modules.insert(name, code);
        }
        let result = self.vm.execute_module_graph(key, &self.modules);
        if result.is_err() {
            // One module that cannot link must not keep the next from running.
            for name in &added {
                self.modules.remove(name);
            }
        }
        let _ = self.vm.run_host_microtask_checkpoint();
        self.collect_console();
        result.map(|_| ()).map_err(|e| {
            let message = format!("{key}: {e}");
            self.note(&message);
            message
        })
    }

    /// Runs every timer that is due and the microtasks they queue.
    pub fn tick(&mut self) -> usize {
        let now = self.now_ms();
        let ran = self
            .vm
            .run_due_host_timers(now)
            .map_or(0, |run| run.tasks_run as usize);
        self.collect_console();
        ran
    }

    /// When the next timer is due, on the realm's clock.
    pub fn next_timer_due(&self) -> Option<u64> {
        self.vm.next_host_timer_due()
    }

    pub fn console(&self) -> &[String] {
        &self.console
    }

    pub fn with_state<R>(&self, f: impl FnOnce(&mut DomState) -> R) -> R {
        f(&mut self.state.borrow_mut())
    }

    fn collect_console(&mut self) {
        if let Ok(batch) = self.vm.drain_host_console() {
            for record in batch.records {
                self.note(&format!("[{}] {}", record.method, record.message));
            }
        }
    }

    fn note(&mut self, line: &str) {
        if self.console.len() >= MAX_CONSOLE_LINES {
            self.console.remove(0);
        }
        self.console.push(line.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(html: &str) -> WebHost {
        WebHost::new(DomState::new(
            blueice_html::parse(html),
            "https://example.com/dir/page?q=1#top",
            (1280.0, 720.0),
        ))
        .unwrap()
    }

    /// Runs `script` and returns what `console.log` printed last.
    fn log(host: &mut WebHost, script: &str) -> String {
        host.console.clear();
        host.run("test", script).unwrap_or_else(|e| panic!("{e}"));
        host.console().last().cloned().unwrap_or_default()
    }

    #[test]
    fn the_platform_installs_and_a_script_reads_the_document() {
        let mut h = host("<title>Hi</title><p id=a class='x y'>text</p>");
        assert_eq!(
            log(&mut h, "console.log(document.getElementById('a').className, document.title, typeof window.location)"),
            "[log] x y Hi object"
        );
    }

    #[test]
    fn scripts_build_the_dom_and_the_host_sees_it() {
        let mut h = host("<div id=root></div>");
        h.run(
            "build",
            "const r = document.getElementById('root');
             const ul = document.createElement('ul');
             for (const t of ['a','b']) { const li = document.createElement('li'); li.textContent = t; li.classList.add('item'); ul.appendChild(li); }
             r.appendChild(ul);
             r.setAttribute('data-n', '2');",
        )
        .unwrap();
        h.with_state(|s| {
            assert!(s.dirty);
            let root = s.call("byId", &[Reply::Text("root".into())]).unwrap();
            assert_eq!(
                s.call("innerHTML", &[root]).unwrap(),
                Reply::Text("<ul><li class=\"item\">a</li><li class=\"item\">b</li></ul>".into())
            );
        });
    }

    #[test]
    fn query_selectors_wrap_the_same_node_every_time() {
        let mut h = host("<ul><li class=a>1</li><li class=a>2</li></ul>");
        assert_eq!(
            log(&mut h, "const a = document.querySelectorAll('li.a'); console.log(a.length, a[0] === document.querySelector('.a'), a[1].textContent, a[0].nextElementSibling === a[1])"),
            "[log] 2 true 2 true"
        );
    }

    #[test]
    fn events_capture_target_and_bubble_in_order_and_can_be_stopped() {
        let mut h = host("<div id=o><button id=b>go</button></div>");
        let out = log(
            &mut h,
            "const order = [];
             const o = document.getElementById('o'), b = document.getElementById('b');
             document.addEventListener('click', () => order.push('doc'));
             o.addEventListener('click', () => order.push('o-capture'), true);
             o.addEventListener('click', () => order.push('o'));
             b.addEventListener('click', (e) => { order.push('b'); });
             b.click();
             o.addEventListener('click', (e) => e.stopPropagation());
             order.push('|');
             b.click();
             console.log(order.join(','));",
        );
        assert_eq!(out, "[log] o-capture,b,o,doc,|,o-capture,b,o");
    }

    #[test]
    fn once_listeners_and_default_prevention() {
        let mut h = host("<a id=a href='#'>x</a>");
        assert_eq!(
            log(&mut h, "let n = 0; const a = document.getElementById('a');
             a.addEventListener('x', () => n++, { once: true });
             const e = new Event('x', { cancelable: true });
             a.addEventListener('x', (ev) => ev.preventDefault());
             const r1 = a.dispatchEvent(e); a.dispatchEvent(new Event('x'));
             console.log(n, r1, e.defaultPrevented)"),
            "[log] 1 false true"
        );
    }

    #[test]
    fn timers_run_through_the_host_tick() {
        let mut h = host("<p id=p>0</p>");
        h.run("t", "let n = 0; setTimeout(() => { document.getElementById('p').textContent = 'done'; }, 0); requestAnimationFrame(() => console.log('frame'));").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(40));
        assert!(h.tick() >= 1);
        h.with_state(|s| {
            let p = s.call("byId", &[Reply::Text("p".into())]).unwrap();
            assert_eq!(s.call("textContent", &[p]).unwrap(), Reply::Text("done".into()));
        });
        assert!(h.console().iter().any(|l| l.contains("frame")));
    }

    #[test]
    fn a_failing_script_reports_and_the_next_still_runs() {
        let mut h = host("<p></p>");
        assert!(h.run("bad", "throw new Error('boom')").is_err());
        assert!(h.run("syntax", "var = ;").is_err());
        assert_eq!(log(&mut h, "console.log('still here')"), "[log] still here");
    }

    #[test]
    fn location_history_and_urls_describe_the_document() {
        let mut h = host("<p></p>");
        assert_eq!(
            log(&mut h, "console.log(location.pathname, location.search, location.hash, location.origin, new URL('../x', location.href).href, new URLSearchParams('a=1&b=2').get('b'))"),
            "[log] /dir/page ?q=1 #top https://example.com https://example.com/x 2"
        );
        h.run("push", "history.pushState({ n: 1 }, '', '/dir/next'); ").unwrap();
        assert_eq!(log(&mut h, "console.log(location.pathname, JSON.stringify(history.state), history.length)"), "[log] /dir/next {\"n\":1} 2");
        h.with_state(|s| {
            assert!(matches!(s.navigations.first(), Some(dom_bridge::NavigationRequest::PushState { .. })));
        });
    }

    #[test]
    fn pushstate_to_another_origin_is_refused() {
        let mut h = host("");
        assert_eq!(
            log(&mut h, "try { history.pushState(null, '', 'https://evil.example/') } catch (e) { console.log(e.name) }"),
            "[log] SecurityError"
        );
    }

    #[test]
    fn popstate_fires_when_going_back_inside_the_document() {
        let mut h = host("");
        h.run("setup", "history.pushState({ p: 1 }, '', '/one'); window.addEventListener('popstate', (e) => console.log('pop', JSON.stringify(e.state))); history.back();").unwrap();
        h.tick();
        assert!(h.console().iter().any(|l| l == "[log] pop null"), "{:?}", h.console());
        assert_eq!(log(&mut h, "console.log(location.pathname)"), "[log] /dir/page");
    }

    #[test]
    fn mutation_observers_see_child_list_and_attribute_changes() {
        let mut h = host("<div id=d></div>");
        h.run(
            "m",
            "const d = document.getElementById('d');
             new MutationObserver((records) => console.log(records.map(r => r.type + ':' + (r.attributeName || r.addedNodes.length)).join(' '))).observe(d, { childList: true, attributes: true });
             d.appendChild(document.createElement('i')); d.setAttribute('title', 't');",
        )
        .unwrap();
        assert!(h.console().iter().any(|l| l == "[log] childList:1 attributes:title"), "{:?}", h.console());
    }

    #[test]
    fn class_list_dataset_and_style_read_and_write_attributes() {
        let mut h = host("<p id=p class='a b' data-foo-bar=1 style='color: red'></p>");
        assert_eq!(
            log(&mut h, "const p = document.getElementById('p');
              p.classList.toggle('a'); p.classList.add('c'); p.dataset.fooBar = '2'; p.dataset.baz = 'z'; p.style.backgroundColor = 'blue'; p.style.setProperty('margin-top', '3px');
              console.log(p.className, p.getAttribute('data-foo-bar'), p.dataset.baz, p.getAttribute('style'), p.style.color)"),
            "[log] b c 2 z color: red; background-color: blue; margin-top: 3px; red"
        );
    }

    #[test]
    fn inner_html_scripts_templates_and_fragments_work() {
        let mut h = host("<div id=d></div><template id=t><b>x</b></template>");
        assert_eq!(
            log(&mut h, "const d = document.getElementById('d');
              d.innerHTML = '<p class=n>1</p><p class=n>2</p>';
              const c = document.getElementById('t').content.cloneNode(true);
              d.appendChild(c);
              d.insertAdjacentHTML('afterbegin', '<h1>t</h1>');
              console.log(d.children.length, d.firstElementChild.tagName, d.querySelectorAll('.n').length, d.lastElementChild.outerHTML)"),
            "[log] 4 H1 2 <b>x</b>"
        );
    }

    #[test]
    fn custom_elements_upgrade_existing_and_new_elements() {
        let mut h = host("<x-box id=a data-v=1></x-box>");
        assert_eq!(
            log(&mut h, "class XBox extends HTMLElement {
                 static get observedAttributes() { return ['data-v']; }
                 connectedCallback() { this.dataset.connected = 'yes'; }
                 attributeChangedCallback(name, old, now) { console.log('attr', name, old, now); }
                 hello() { return 'hi ' + this.id; }
               }
               customElements.define('x-box', XBox);
               const a = document.getElementById('a');
               const b = new XBox(); b.id = 'b'; document.body.appendChild(b);
               const c = document.createElement('x-box'); c.id = 'c';
               console.log(a.hello(), a.dataset.connected, b.hello(), b.dataset.connected, c instanceof XBox)"),
            "[log] hi a yes hi b yes true"
        );
        assert!(h.console().iter().any(|l| l == "[log] attr data-v null 1"));
    }

    #[test]
    fn dom_parser_and_matches_and_closest() {
        let mut h = host("<section class=s><p id=p></p></section>");
        assert_eq!(
            log(&mut h, "const doc = new DOMParser().parseFromString('<html><head><title>T</title></head><body><main id=m>hi</main></body></html>', 'text/html');
              const p = document.getElementById('p');
              console.log(doc.title, doc.querySelector('#m').textContent, doc.body.children.length, p.closest('.s').tagName, p.matches('section > p'))"),
            "[log] T hi 1 SECTION true"
        );
    }

    #[test]
    fn web_apis_pages_expect_exist_and_work() {
        let mut h = host("");
        assert_eq!(
            log(&mut h, "console.log(btoa('hello'), atob('aGVsbG8='), new TextEncoder().encode('é').length, new TextDecoder().decode(new TextEncoder().encode('日本')), JSON.stringify(structuredClone({ a: [1, { b: 2 }] })), typeof localStorage.getItem('x'), CSS.escape('a.b'), matchMedia('(min-width: 600px)').matches, typeof performance.now(), navigator.language)"),
            "[log] aGVsbG8= hello 2 日本 {\"a\":[1,{\"b\":2}]} object a\\.b true number en-US"
        );
    }

    #[test]
    fn storage_and_cookies_persist_for_the_page() {
        let mut h = host("");
        assert_eq!(
            log(&mut h, "localStorage.setItem('k', 'v'); localStorage.other = 'o'; document.cookie = 'a=1; path=/'; document.cookie = 'b=2';
              console.log(localStorage.getItem('k'), localStorage.length, localStorage.other, document.cookie)"),
            "[log] v 2 o a=1; b=2"
        );
    }

    #[test]
    fn intersection_observers_report_visible_so_reveal_on_scroll_runs() {
        let mut h = host("<div id=d class=hidden></div>");
        h.run("io", "const d = document.getElementById('d'); new IntersectionObserver((entries, obs) => { entries.forEach(e => { if (e.isIntersecting) { e.target.classList.remove('hidden'); e.target.classList.add('shown'); obs.unobserve(e.target); } }); }).observe(d);").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.tick();
        h.with_state(|s| {
            let d = s.call("byId", &[Reply::Text("d".into())]).unwrap();
            assert_eq!(s.call("getAttr", &[d, Reply::Text("class".into())]).unwrap(), Reply::Text("shown".into()));
        });
    }
}
