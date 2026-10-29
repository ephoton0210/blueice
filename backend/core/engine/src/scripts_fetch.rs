// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The external scripts of a page whose HTML has cleared both gatekeeper
//! stages, fetched on the navigation's background thread behind the same
//! per-hop URL review as stylesheets and images. A script is optional content:
//! one that fails (review, network, status, size) is simply not run, and the
//! page continues with the rest.

use std::collections::HashMap;
use url::Url;

/// Script source by the `src` as written.
pub type LinkedScripts = HashMap<String, String>;

/// Module source by module key (see [`module_key`]).
pub type LinkedModules = HashMap<String, String>;

#[derive(Clone, Copy)]
pub(crate) struct ScriptLimits {
    pub scripts: usize,
    pub bytes_each: usize,
    pub bytes_total: usize,
    pub redirects: usize,
}

pub(crate) const DEFAULT_SCRIPT_LIMITS: ScriptLimits = ScriptLimits {
    scripts: 96,
    bytes_each: 4 * 1024 * 1024,
    bytes_total: 24 * 1024 * 1024,
    redirects: 3,
};

/// The `src` of every external `<script>`, as written, in document order.
pub(crate) fn script_sources(doc: &blueice_dom::Document) -> Vec<String> {
    use blueice_dom::{NodeData, NodeId};
    fn walk(doc: &blueice_dom::Document, node: NodeId, out: &mut Vec<String>) {
        if let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            if tag_name == "script" {
                if let Some((_, src)) = attributes.iter().find(|(k, _)| k == "src") {
                    let src = src.trim();
                    if !src.is_empty() && !out.iter().any(|s| s == src) {
                        out.push(src.to_string());
                    }
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

/// Fetches the external scripts of `html`. `review` answers whether one
/// absolute URL may be requested.
pub(crate) fn fetch_linked_scripts(
    html: &str,
    document_url: &str,
    review: &(dyn Fn(&str) -> bool + Sync),
    limits: ScriptLimits,
) -> LinkedScripts {
    let doc = blueice_html::parse(html);
    let Some(base) = crate::linked_styles::base_url(&doc, document_url) else {
        return LinkedScripts::new();
    };
    let wanted: Vec<(String, String)> = script_sources(&doc)
        .into_iter()
        .filter_map(|src| {
            let absolute = base.join(&src).ok()?;
            matches!(absolute.scheme(), "http" | "https").then(|| (src, absolute.to_string()))
        })
        .take(limits.scripts)
        .collect();
    let fetched: Vec<Option<String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = wanted
            .iter()
            .map(|(_, url)| scope.spawn(move || fetch_one(url, review, limits)))
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().ok().flatten())
            .collect()
    });
    let mut total = 0;
    let mut scripts = LinkedScripts::new();
    for ((src, _), text) in wanted.into_iter().zip(fetched) {
        let Some(text) = text else { continue };
        total += text.len();
        if total > limits.bytes_total {
            break;
        }
        scripts.insert(src, text);
    }
    scripts
}

/// Fetches one script URL (following reviewed redirects), or `None`.
pub(crate) fn fetch_one(
    start: &str,
    review: &(dyn Fn(&str) -> bool + Sync),
    limits: ScriptLimits,
) -> Option<String> {
    let mut url = start.to_string();
    for _ in 0..=limits.redirects {
        if !matches!(Url::parse(&url).ok()?.scheme(), "http" | "https") || !review(&url) {
            return None;
        }
        match blueice_net::fetch_subresource_hop(&url, limits.bytes_each).ok()? {
            blueice_net::SubresourceHop::Redirect { location, .. } => url = location,
            blueice_net::SubresourceHop::Response { status, body, .. } => {
                return (200..300)
                    .contains(&status)
                    .then(|| String::from_utf8_lossy(&body).into_owned());
            }
            blueice_net::SubresourceHop::TooLarge => return None,
        }
    }
    None
}

/// The name a module goes by inside the realm: its path (and query) when it
/// is served by the document's own origin, so that `./x.js`, `../x.js` and
/// `/x.js` all name the same module; otherwise its whole URL.
pub(crate) fn module_key(url: &Url, document: &Url) -> String {
    if url.origin() == document.origin() {
        match url.query() {
            Some(q) => format!("{}?{q}", url.path()),
            None => url.path().to_string(),
        }
    } else {
        let mut u = url.clone();
        u.set_fragment(None);
        u.to_string()
    }
}

/// How the engine turns an import specifier into a key, relative to the
/// importing module's key (a relative path resolves against its directory).
pub(crate) fn resolve_key(referrer: &str, request: &str) -> String {
    if request.starts_with("./") || request.starts_with("../") {
        let mut parts: Vec<&str> = referrer.split('/').collect();
        if parts.len() > 1 {
            parts.pop();
        } else {
            parts.clear();
        }
        for part in request.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                part => parts.push(part),
            }
        }
        return parts.join("/");
    }
    request.to_string()
}

/// `import("literal")` targets named in script text.
pub(crate) fn dynamic_import_literals(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("import(") {
        let after = rest[at + 7..].trim_start();
        if let Some(quote) = after
            .chars()
            .next()
            .filter(|c| matches!(c, '"' | '\'' | '`'))
        {
            if let Some(end) = after[1..].find(quote) {
                let spec = &after[1..1 + end];
                if !spec.is_empty() && !spec.contains("${") && !spec.contains('\n') {
                    out.push(spec.to_string());
                }
            }
        }
        rest = &rest[at + 7..];
    }
    out
}

/// The module URLs the document names itself: `<script type=module src>`,
/// `modulepreload` links, and the `*-url` attributes custom elements (such as
/// Astro islands) carry for the module they will import.
fn module_seeds(doc: &blueice_dom::Document, base: &Url) -> Vec<Url> {
    use blueice_dom::{NodeData, NodeId};
    fn walk(doc: &blueice_dom::Document, node: NodeId, base: &Url, out: &mut Vec<Url>) {
        if let NodeData::Element {
            tag_name,
            attributes,
        } = doc.data(node)
        {
            let attr = |name: &str| {
                attributes
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.trim())
            };
            let mut add = |value: &str| {
                if let Ok(url) = base.join(value) {
                    if matches!(url.scheme(), "http" | "https") && !out.contains(&url) {
                        out.push(url);
                    }
                }
            };
            if tag_name == "script"
                && attr("type").is_some_and(|t| t.eq_ignore_ascii_case("module"))
            {
                if let Some(src) = attr("src").filter(|s| !s.is_empty()) {
                    add(src);
                }
            }
            if tag_name == "link"
                && attr("rel").is_some_and(|r| {
                    r.split_whitespace()
                        .any(|t| t.eq_ignore_ascii_case("modulepreload"))
                })
            {
                if let Some(href) = attr("href").filter(|s| !s.is_empty()) {
                    add(href);
                }
            }
            if tag_name.contains('-') {
                for (name, value) in attributes {
                    if name.ends_with("-url") && !value.trim().is_empty() {
                        add(value.trim());
                    }
                }
            }
        }
        for child in doc.children(node) {
            walk(doc, child, base, out);
        }
    }
    let mut out = Vec::new();
    walk(doc, doc.root(), base, &mut out);
    out
}

/// Limits for a page's module graph.
#[derive(Clone, Copy)]
pub(crate) struct ModuleLimits {
    pub modules: usize,
    pub bytes_each: usize,
    pub bytes_total: usize,
    pub rounds: usize,
    pub redirects: usize,
}

pub(crate) const DEFAULT_MODULE_LIMITS: ModuleLimits = ModuleLimits {
    modules: 300,
    bytes_each: 4 * 1024 * 1024,
    bytes_total: 40 * 1024 * 1024,
    rounds: 12,
    redirects: 3,
};

/// Fetches the page's module graph: its module scripts, preloads and island
/// modules, then everything those import statically or name in a literal
/// `import()`, round by round up to the limits. A module that cannot be
/// fetched is left out; whatever imports it then fails to link.
pub(crate) fn fetch_linked_modules(
    html: &str,
    classic: &LinkedScripts,
    document_url: &str,
    review: &(dyn Fn(&str) -> bool + Sync),
    limits: ModuleLimits,
) -> LinkedModules {
    let doc = blueice_html::parse(html);
    let Some(base) = crate::linked_styles::base_url(&doc, document_url) else {
        return LinkedModules::new();
    };
    let Ok(document) = Url::parse(document_url) else {
        return LinkedModules::new();
    };
    let mut frontier: Vec<Url> = module_seeds(&doc, &base);
    // Literal dynamic imports of inline scripts and fetched classic scripts.
    let mut texts: Vec<(Url, String)> = classic
        .iter()
        .filter_map(|(src, text)| Some((base.join(src).ok()?, text.clone())))
        .collect();
    fn inline_text(doc: &blueice_dom::Document, node: blueice_dom::NodeId, out: &mut Vec<String>) {
        if let blueice_dom::NodeData::Element { tag_name, .. } = doc.data(node) {
            if tag_name == "script" {
                let mut text = String::new();
                for child in doc.children(node) {
                    if let blueice_dom::NodeData::Text { data } = doc.data(child) {
                        text.push_str(data);
                    }
                }
                if !text.is_empty() {
                    out.push(text);
                }
            }
        }
        for child in doc.children(node) {
            inline_text(doc, child, out);
        }
    }
    let mut inline = Vec::new();
    inline_text(&doc, doc.root(), &mut inline);
    texts.extend(inline.into_iter().map(|t| (base.clone(), t)));
    let mut modules = LinkedModules::new();
    let mut seen: std::collections::HashSet<Url> = std::collections::HashSet::new();
    let mut total = 0usize;
    let scan = |origin: &Url, text: &str, frontier: &mut Vec<Url>, module_graph: bool| {
        let mut specs: Vec<String> = dynamic_import_literals(text);
        if module_graph {
            if let Ok(module) = blueice_bluejs::parse_module(text) {
                specs.extend(module.requests.iter().map(|r| r.specifier.clone()));
            }
        }
        for spec in specs {
            if spec.starts_with("./")
                || spec.starts_with("../")
                || spec.starts_with('/')
                || spec.contains("://")
            {
                if let Ok(url) = origin.join(&spec) {
                    if matches!(url.scheme(), "http" | "https") {
                        frontier.push(url);
                    }
                }
            }
        }
    };
    for (origin, text) in &texts {
        scan(origin, text, &mut frontier, false);
    }
    for _ in 0..limits.rounds {
        let wanted: Vec<Url> = std::mem::take(&mut frontier)
            .into_iter()
            .filter(|u| seen.insert(u.clone()))
            .take(limits.modules.saturating_sub(modules.len()))
            .collect();
        if wanted.is_empty() {
            break;
        }
        let fetch_limits = ScriptLimits {
            scripts: limits.modules,
            bytes_each: limits.bytes_each,
            bytes_total: limits.bytes_total,
            redirects: limits.redirects,
        };
        let fetched: Vec<Option<String>> = std::thread::scope(|scope| {
            let workers: Vec<_> = wanted
                .iter()
                .map(|url| scope.spawn(move || fetch_one(url.as_str(), review, fetch_limits)))
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().ok().flatten())
                .collect()
        });
        for (url, text) in wanted.into_iter().zip(fetched) {
            let Some(text) = text else { continue };
            total += text.len();
            if total > limits.bytes_total {
                return modules;
            }
            scan(&url, &text, &mut frontier, true);
            modules.insert(module_key(&url, &document), text);
        }
    }
    modules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_come_from_external_scripts_only_once_each() {
        let doc = blueice_html::parse(
            "<script src='a.js'></script><script>inline()</script><script src=' b.js ' type=module></script><script src='a.js'></script><script src=''></script>",
        );
        assert_eq!(script_sources(&doc), vec!["a.js", "b.js"]);
    }

    #[test]
    fn a_refused_review_fetches_nothing() {
        let html = "<script src='http://127.0.0.1:1/x.js'></script>";
        let scripts = fetch_linked_scripts(
            html,
            "http://127.0.0.1:1/",
            &|_| false,
            DEFAULT_SCRIPT_LIMITS,
        );
        assert!(scripts.is_empty());
    }

    #[test]
    fn module_keys_unify_same_origin_spellings() {
        let doc = Url::parse("https://h.example/dir/page").unwrap();
        let a = Url::parse("https://h.example/_astro/a.js").unwrap();
        let cdn = Url::parse("https://cdn.example/x/y.js#frag").unwrap();
        assert_eq!(module_key(&a, &doc), "/_astro/a.js");
        assert_eq!(module_key(&cdn, &doc), "https://cdn.example/x/y.js");
        assert_eq!(resolve_key("/_astro/a.js", "./b.js"), "/_astro/b.js");
        assert_eq!(resolve_key("/_astro/a.js", "../c.js"), "/c.js");
        assert_eq!(resolve_key("/_astro/a.js", "/d.js"), "/d.js");
        assert_eq!(
            resolve_key("https://cdn.example/x/y.js", "./z.js"),
            "https://cdn.example/x/z.js"
        );
    }

    #[test]
    fn literal_dynamic_imports_are_found_and_computed_ones_are_not() {
        let found = dynamic_import_literals("a(); import('./a.js'); await import( \"/b.js\" ); import(x); import(`./t.js`); import(`./${n}.js`)");
        assert_eq!(found, vec!["./a.js", "/b.js", "./t.js"]);
    }

    #[test]
    fn the_document_names_its_own_module_seeds() {
        let doc = blueice_html::parse(
            "<script type=module src='/a.js'></script><script src='/classic.js'></script>\
             <link rel='modulepreload' href='/p.js'><astro-island component-url='/c.js' renderer-url='/r.js'></astro-island><p data-url='/no.js'></p>",
        );
        let base = Url::parse("https://h.example/").unwrap();
        let urls: Vec<String> = module_seeds(&doc, &base)
            .iter()
            .map(|u| u.path().to_string())
            .collect();
        assert_eq!(urls, vec!["/a.js", "/p.js", "/c.js", "/r.js"]);
    }
}
