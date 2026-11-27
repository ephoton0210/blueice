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

/// The chunk files a webpack-built page will `import()` under computed names
/// (`import("./" + __webpack_require__.u(id))`), which no literal scan can see.
/// `scripts` are `(url, text)` pairs of the page's scripts: the one holding the
/// chunk-filename table (`{7489:"879b548e785958fd", ...}` beside `".js"`) says what
/// each chunk id is called, and a context map (`"./dropdown/element.js":["58623",[2868]]`)
/// says which chunks a custom element's code lives in. Only the elements the page's own
/// HTML uses (`<dropdown>`, `<mdn-dropdown>`) have their chunks wanted.
pub(crate) fn webpack_chunk_urls(scripts: &[(Url, String)], html: &str) -> Vec<Url> {
    let mut tags: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for piece in html.split('<').skip(1) {
        let end = piece
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .unwrap_or(piece.len());
        if end > 0 {
            tags.insert(&piece[..end]);
        }
    }
    let mut hashes: std::collections::HashMap<String, (String, &Url)> = Default::default();
    for (url, text) in scripts {
        if !text.contains("\".js\"") {
            continue;
        }
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            // `<digits>:"<16 hex>"`
            if bytes[i].is_ascii_digit() && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if text[j..].starts_with(":\"") {
                    let hash = &text[j + 2..];
                    if hash.len() > 17
                        && hash.as_bytes()[16] == b'"'
                        && hash[..16].bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        hashes.insert(text[i..j].to_string(), (hash[..16].to_string(), url));
                    }
                }
                i = j.max(i + 1);
            } else {
                i += 1;
            }
        }
    }
    if hashes.len() < 8 {
        return Vec::new();
    }
    let mut wanted: Vec<String> = Vec::new();
    for (_, text) in scripts {
        let mut rest = text.as_str();
        while let Some(at) = rest.find("/element.js\":[\"") {
            let before = &rest[..at];
            let dir = before
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string();
            let after = &rest[at + 16..];
            if let (Some(open), Some(close)) = (after.find('['), after.find("]]")) {
                if open < close
                    && tags
                        .iter()
                        .any(|t| *t == dir || t.ends_with(&format!("-{dir}")))
                {
                    for id in after[open + 1..close].split(',') {
                        let id = id.trim();
                        if !id.is_empty() && !wanted.iter().any(|w| w == id) {
                            wanted.push(id.to_string());
                        }
                    }
                }
            }
            rest = &rest[at + 16..];
        }
    }
    wanted
        .iter()
        .filter_map(|id| {
            let (hash, runtime) = hashes.get(id)?;
            runtime.join(&format!("{id}.{hash}.js")).ok()
        })
        .collect()
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

/// The page's import map: `<script type="importmap">`'s `imports`, each address resolved
/// against the document base. Entries whose address is not an http(s) URL are dropped.
pub(crate) fn import_map(doc: &blueice_dom::Document, base: &Url) -> Vec<(String, Url)> {
    use blueice_dom::{NodeData, NodeId};
    fn walk(doc: &blueice_dom::Document, node: NodeId, base: &Url, out: &mut Vec<(String, Url)>) {
        if let NodeData::Element { tag_name, attributes } = doc.data(node) {
            let importmap = tag_name == "script"
                && attributes
                    .iter()
                    .any(|(k, v)| k == "type" && v.trim().eq_ignore_ascii_case("importmap"));
            if importmap {
                let mut text = String::new();
                for child in doc.children(node) {
                    if let NodeData::Text { data } = doc.data(child) {
                        text.push_str(data);
                    }
                }
                if let Ok(serde_json::Value::Object(root)) = serde_json::from_str(&text) {
                    if let Some(serde_json::Value::Object(imports)) = root.get("imports") {
                        for (name, address) in imports {
                            let Some(address) = address.as_str() else { continue };
                            if let Ok(url) = base.join(address) {
                                if matches!(url.scheme(), "http" | "https") {
                                    out.push((name.clone(), url));
                                }
                            }
                        }
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

/// A module's own `import.meta.url` as the string literal of its address. BlueJS gives
/// `import.meta` no fields, and bundlers (webpack's automatic public path, `new URL(x,
/// import.meta.url)`) cannot run without it; the engine fetched the module, so it knows the URL.
pub(crate) fn bind_import_meta_url(text: String, url: &Url) -> String {
    if !text.contains("import.meta.url") {
        return text;
    }
    let mut u = url.clone();
    u.set_fragment(None);
    let literal = serde_json::Value::String(u.to_string()).to_string();
    text.replace("import.meta.url", &literal)
}

/// What the import map says a bare specifier means: an exact entry, or the longest
/// trailing-slash entry that prefixes it. Relative, absolute and URL specifiers are not mapped.
pub(crate) fn mapped_specifier(map: &[(String, Url)], specifier: &str) -> Option<Url> {
    if specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with('/')
        || specifier.contains("://")
    {
        return None;
    }
    if let Some((_, url)) = map.iter().find(|(name, _)| name == specifier) {
        return Some(url.clone());
    }
    map.iter()
        .filter(|(name, _)| name.ends_with('/') && specifier.starts_with(name.as_str()))
        .max_by_key(|(name, _)| name.len())
        .and_then(|(name, url)| url.join(&specifier[name.len()..]).ok())
}

/// The module that stands under a mapped bare specifier's name: it re-exports everything of
/// the real module (`key`), so the real one is still evaluated once however it is named.
pub(crate) fn alias_source(key: &str, target: Option<&str>) -> String {
    let has_default = target
        .and_then(|text| blueice_bluejs::parse_module(text).ok())
        .is_some_and(|module| {
            module.exports.iter().any(|entry| {
                matches!(entry,
                    blueice_bluejs::ExportEntry::Local { export_name, .. }
                    | blueice_bluejs::ExportEntry::Indirect { export_name, .. }
                    if export_name == "default")
            })
        });
    let mut source = format!("export * from \"{key}\";\n");
    if has_default {
        source.push_str(&format!("export {{ default }} from \"{key}\";\n"));
    }
    source
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
    let map = import_map(&doc, &base);
    let aliases = std::cell::RefCell::new(Vec::<(String, Url)>::new());
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
            if let Some(url) = mapped_specifier(&map, &spec) {
                frontier.push(url.clone());
                aliases.borrow_mut().push((spec, url));
                continue;
            }
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
    frontier.extend(
        webpack_chunk_urls(&texts, html)
            .into_iter()
            .filter(|u| matches!(u.scheme(), "http" | "https")),
    );
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
                return finish_aliases(modules, &aliases.borrow(), &document);
            }
            let text = bind_import_meta_url(text, &url);
            scan(&url, &text, &mut frontier, true);
            modules.insert(module_key(&url, &document), text);
        }
    }
    let result = finish_aliases(modules, &aliases.borrow(), &document);
    result
}

/// Adds, under each mapped bare specifier's own name, a module re-exporting the real one.
fn finish_aliases(
    mut modules: LinkedModules,
    aliases: &[(String, Url)],
    document: &Url,
) -> LinkedModules {
    for (name, url) in aliases {
        let key = module_key(url, document);
        if modules.contains_key(&key) && !modules.contains_key(name) {
            let shim = alias_source(&key, modules.get(&key).map(String::as_str));
            modules.insert(name.clone(), shim);
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
    fn webpack_chunks_of_the_elements_a_page_uses_are_wanted() {
        let runtime = Url::parse("https://x.test/static/client/runtime.js").unwrap();
        let table: String = (1..=9)
            .map(|n| format!("{}:\"{:016x}\"", 100 + n, n))
            .collect::<Vec<_>>()
            .join(",");
        let scripts = vec![
            (runtime, format!("o.u=e=>\"\"+e+\".\"+{{{table}}}[e]+\".js\"")),
            (
                Url::parse("https://x.test/static/client/index.js").unwrap(),
                r#"var a={"./dropdown/element.js":["58623",[102,103]],"./gone/element.js":["1",[104]]}"#.into(),
            ),
        ];
        let urls = webpack_chunk_urls(&scripts, "<body><mdn-dropdown></mdn-dropdown></body>");
        let urls: Vec<String> = urls.iter().map(|u| u.to_string()).collect();
        assert_eq!(
            urls,
            vec![
                "https://x.test/static/client/102.0000000000000002.js".to_string(),
                "https://x.test/static/client/103.0000000000000003.js".to_string()
            ],
            "only the chunks of an element the page has, named by the filename table"
        );
        assert!(webpack_chunk_urls(&scripts, "<body></body>").is_empty());
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

    #[test]
    fn an_import_map_names_bare_specifiers_and_prefixes() {
        let doc = blueice_html::parse(
            r#"<script type="importmap">{"imports":{"react":"https://cdn.example/react-1.js","react-dom/client":"/assets/rdc.js","lib/":"https://cdn.example/lib/"}}</script>"#,
        );
        let base = Url::parse("https://h.example/p").unwrap();
        let map = import_map(&doc, &base);
        let target = |spec: &str| mapped_specifier(&map, spec).map(|u| u.to_string());
        assert_eq!(target("react").as_deref(), Some("https://cdn.example/react-1.js"));
        assert_eq!(target("react-dom/client").as_deref(), Some("https://h.example/assets/rdc.js"));
        assert_eq!(target("lib/a/b.js").as_deref(), Some("https://cdn.example/lib/a/b.js"));
        assert_eq!(target("vue"), None);
        // Relative and absolute specifiers are never the map's business here.
        assert_eq!(target("./react"), None);
    }

    #[test]
    fn an_import_map_alias_re_exports_the_mapped_module() {
        let with_default = "export default 1; export const a = 2;";
        let without = "export const a = 2;";
        let shim = alias_source("/assets/m.js", Some(with_default));
        assert!(shim.contains("export * from \"/assets/m.js\""));
        assert!(shim.contains("export { default } from \"/assets/m.js\""));
        let shim = alias_source("/assets/m.js", Some(without));
        assert!(!shim.contains("default"));
    }

    #[test]
    fn a_fetched_module_sees_its_own_address_as_import_meta_url() {
        let url = Url::parse("https://cdn.example/a/b.js#x").unwrap();
        assert_eq!(
            bind_import_meta_url("const u = new URL('./c.js', import.meta.url);".into(), &url),
            "const u = new URL('./c.js', \"https://cdn.example/a/b.js\");"
        );
        assert_eq!(bind_import_meta_url("noop()".into(), &url), "noop()");
    }
}
