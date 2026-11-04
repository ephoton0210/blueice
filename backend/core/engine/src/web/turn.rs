// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What a script turn can be. A turn used to be a closure the page handed the
//! realm; with the realm in another thread, and then another process, it has to
//! be plain data that says what to run. [`run_turn`] is the one place that
//! turns that data into running the realm, so every worker runs a turn the same
//! way.

use super::WebHost;
use std::collections::HashMap;
use std::time::Duration;

/// How long a document's scripts may spend starting up before the rest are
/// left unrun, so one heavy page cannot hold the session for long.
pub(crate) const STARTUP_BUDGET: Duration = Duration::from_secs(6);

/// A script element of the document, as the startup turn needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptPlan {
    /// The element's node ID.
    pub node: u64,
    pub module: bool,
    pub src: Option<String>,
    pub inline: String,
    pub deferred: bool,
}

/// Everything the startup turn runs: the document's scripts in the order the
/// page found them, what was fetched for the external ones, and the address the
/// document came from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Startup {
    pub scripts: Vec<ScriptPlan>,
    /// External classic scripts by the `src` the element has.
    pub linked: HashMap<String, String>,
    /// Module sources by module key.
    pub modules: HashMap<String, String>,
    pub document: Option<String>,
}

/// One turn of a document's scripts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Turn {
    /// Every script of the document, then `DOMContentLoaded` and `load`.
    Startup(Startup),
    /// A script a script inserted has been fetched (or has failed to be).
    ScriptLoaded {
        node: u64,
        src: String,
        source: Option<String>,
    },
    /// Layout has caught up with a change; observers of visibility report.
    AfterLayout,
    /// The page scrolled to this offset.
    Scrolled(f64),
    /// Script source to run for each finished network request.
    NetCalls(Vec<String>),
    /// Run the timers that are due.
    Tick,
    /// Run this source as a diagnostic probe.
    Probe(String),
}

/// `text` as a JavaScript string literal.
pub(crate) fn js_string(text: &str) -> String {
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

/// Runs `turn` on the realm.
pub(crate) fn run_turn(host: &mut WebHost, turn: Turn) {
    match turn {
        Turn::Startup(startup) => run_startup(host, startup),
        Turn::ScriptLoaded { node, src, source } => {
            let trace = std::env::var_os("BI_NET_LOG").is_some();
            let ok = source.is_some();
            let call = format!(
                "__blueice.scriptLoaded({node}, {}, {ok});",
                js_string(&source.unwrap_or_default())
            );
            let started = std::time::Instant::now();
            let outcome = host.run("blueice:script-load", &call);
            if trace {
                eprintln!(
                    "script-load {src} ran in {:?}: {outcome:?}",
                    started.elapsed()
                );
            }
        }
        Turn::AfterLayout => {
            let _ = host.run("blueice:layout", "__blueice.afterLayout();");
        }
        Turn::Scrolled(y) => {
            let _ = host.run("blueice:scroll", &format!("__blueice.scrolled({y});"));
        }
        Turn::NetCalls(calls) => {
            for call in &calls {
                let _ = host.run("blueice:net", call);
            }
        }
        Turn::Tick => {
            host.tick();
        }
        Turn::Probe(source) => {
            let _ = host.run("probe", &source);
        }
    }
}

fn run_startup(host: &mut WebHost, startup: Startup) {
    let Startup {
        scripts,
        linked,
        modules,
        document,
    } = startup;
    let began = std::time::Instant::now();
    let document = document.and_then(|u| url::Url::parse(&u).ok());
    let document_dir = document
        .as_ref()
        .map(|u| {
            let path = u.path();
            path.rsplit_once('/').map_or("", |(dir, _)| dir).to_string()
        })
        .unwrap_or_default();
    // Developer aid: `BI_PRELUDE` is JavaScript run before any of the page's scripts.
    if let Ok(prelude) = std::env::var("BI_PRELUDE") {
        let _ = host.run("blueice:prelude", &prelude);
    }
    let compiled = std::time::Instant::now();
    host.set_module_sources(
        &document
            .as_ref()
            .map(|u| crate::scripts_fetch::module_key(u, u))
            .unwrap_or_default(),
        modules.clone(),
    );
    if std::env::var("BI_DIAG").is_ok() {
        eprintln!(
            "DIAG {} modules compiled in {:?}",
            modules.len(),
            compiled.elapsed()
        );
    }
    // Parser-blocking and async classic scripts in order, then the
    // deferred ones, then modules (which are deferred by nature).
    let (module_scripts, classic): (Vec<_>, Vec<_>) = scripts.into_iter().partition(|s| s.module);
    let (deferred, immediate): (Vec<_>, Vec<_>) = classic.into_iter().partition(|s| s.deferred);
    for script in immediate.into_iter().chain(deferred) {
        if began.elapsed() > STARTUP_BUDGET {
            host.note_line("page scripts stopped: the startup time budget is spent");
            break;
        }
        let (name, source) = match &script.src {
            Some(src) => match linked.get(src) {
                Some(text) => (src.clone(), text.clone()),
                None => continue,
            },
            None => ("inline script".to_string(), script.inline),
        };
        let id = script.node;
        let _ = host.run(
            "blueice:current",
            &format!("__blueice.setCurrentScript({id});"),
        );
        let ran = std::time::Instant::now();
        let logged = host.console().len();
        let outcome = host.run(&name, &source);
        // Developer aid: which script said or failed what (`BI_NET_LOG=1`).
        if std::env::var_os("BI_NET_LOG").is_some() {
            eprintln!(
                "script {} ran in {:?}: {outcome:?}",
                &name[..name.len().min(110)],
                ran.elapsed()
            );
            for line in host.console().iter().skip(logged) {
                eprintln!("    | {}", &line[..line.len().min(240)]);
            }
        }
        if std::env::var("BI_DIAG").is_ok() && ran.elapsed().as_millis() > 100 {
            eprintln!(
                "DIAG classic {} took {:?}",
                &name[..name.len().min(70)],
                ran.elapsed()
            );
        }
    }
    for (n, script) in module_scripts.into_iter().enumerate() {
        let key_source = match &script.src {
            Some(src) => document
                .as_ref()
                .and_then(|d| {
                    d.join(src)
                        .ok()
                        .map(|u| (crate::scripts_fetch::module_key(&u, d), src.clone()))
                })
                .and_then(|(key, _)| modules.get(&key).map(|text| (key, text.clone()))),
            None => Some((
                format!("{document_dir}/__inline_module_{n}.js"),
                script.inline.clone(),
            )),
        };
        if began.elapsed() > STARTUP_BUDGET {
            host.note_line("page scripts stopped: the startup time budget is spent");
            break;
        }
        if let Some((key, text)) = key_source {
            let id = script.node;
            let _ = host.run(
                "blueice:current",
                &format!("__blueice.setCurrentScript({id});"),
            );
            let ran = std::time::Instant::now();
            let _ = host.run_module(&key, &text, &modules);
            if std::env::var("BI_DIAG").is_ok() && ran.elapsed().as_millis() > 100 {
                eprintln!(
                    "DIAG module {} took {:?}",
                    &key[..key.len().min(70)],
                    ran.elapsed()
                );
            }
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
}
