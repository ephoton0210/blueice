// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The native half of a page script's DOM: a handful of primitives over the
//! page's own [`Document`] that the JavaScript layer in `bootstrap.js` builds
//! the web's DOM classes on. Each primitive takes and returns only numbers,
//! strings, booleans and `null` (what a host function may carry); a node is
//! the number of its stable `NodeId`.
//!
//! This module is pure Rust over a `Document` so that it can be tested
//! without a JavaScript engine. Nothing here reaches the network or the host:
//! a primitive that needs one (`fetch`, history) only records a request for
//! the host to act on.

use blueice_css::{ComputedStyle, Value};
use blueice_dom::{Document, NodeData, NodeId};
use std::collections::HashMap;

/// Most nodes a script may add to one document.
pub(crate) const MAX_ADDED_NODES: usize = 200_000;

/// A primitive's result.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reply {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
}

impl From<bool> for Reply {
    fn from(v: bool) -> Self {
        Reply::Bool(v)
    }
}
impl From<String> for Reply {
    fn from(v: String) -> Self {
        Reply::Text(v)
    }
}
impl From<&str> for Reply {
    fn from(v: &str) -> Self {
        Reply::Text(v.to_string())
    }
}
impl From<Option<NodeId>> for Reply {
    fn from(v: Option<NodeId>) -> Self {
        v.map_or(Reply::Null, |id| Reply::Number(id.as_u64() as f64))
    }
}
impl From<NodeId> for Reply {
    fn from(v: NodeId) -> Self {
        Reply::Number(v.as_u64() as f64)
    }
}

/// A navigation a script asked for, left for the host to carry out.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NavigationRequest {
    /// `location.assign` / `replace` / `href =`: load this URL.
    Load {
        url: String,
        replace: bool,
    },
    /// `history.pushState` / `replaceState`: the document stays, its URL moves.
    PushState {
        url: String,
        replace: bool,
    },
    Reload,
    /// `history.back` / `forward` / `go`.
    Go(i32),
}

pub(crate) struct DomState {
    pub doc: Document,
    /// Something a script changed needs restyle and layout.
    pub dirty: bool,
    /// Nodes this document has gained from script (against `MAX_ADDED_NODES`).
    added: usize,
    pub styles: HashMap<NodeId, ComputedStyle>,
    /// Border boxes by node, in document coordinates, from the last layout.
    pub rects: HashMap<NodeId, (f64, f64, f64, f64)>,
    pub viewport: (f64, f64),
    pub url: String,
    pub navigations: Vec<NavigationRequest>,
    pub cookies: String,
    /// External scripts a script inserted: `(script element, absolute URL)`.
    pub script_requests: Vec<(NodeId, String)>,
    /// Requests a script made, waiting for the host to run them.
    pub net_jobs: Vec<crate::web::net::NetJob>,
    next_request: u64,
    /// WebGL contexts by id, and the canvases they draw into.
    pub(crate) gl: HashMap<u32, crate::web::gl_bridge::GlContext>,
    next_gl: u32,
    /// Decoded pictures by node (images and canvases), lent by the page.
    pub images: HashMap<NodeId, std::sync::Arc<blueice_image::RasterImage>>,
}

impl DomState {
    pub fn new(doc: Document, url: &str, viewport: (f64, f64)) -> Self {
        DomState {
            doc,
            dirty: false,
            added: 0,
            styles: HashMap::new(),
            rects: HashMap::new(),
            viewport,
            url: url.to_string(),
            navigations: Vec::new(),
            cookies: String::new(),
            script_requests: Vec::new(),
            net_jobs: Vec::new(),
            next_request: 1,
            gl: HashMap::new(),
            next_gl: 1,
            images: HashMap::new(),
        }
    }

    /// Pictures of every canvas drawn to since the last call.
    pub fn take_canvas_pictures(&mut self) -> Vec<(NodeId, std::sync::Arc<blueice_image::RasterImage>)> {
        let mut out = Vec::new();
        for g in self.gl.values_mut().filter(|g| g.dirty) {
            g.dirty = false;
            out.push((
                g.canvas,
                std::sync::Arc::new(blueice_image::RasterImage::new(g.ctx.width, g.ctx.height, g.ctx.picture())),
            ));
        }
        out
    }

    fn node(&self, value: Option<&Reply>) -> Result<NodeId, String> {
        match value {
            Some(Reply::Number(n)) if *n >= 0.0 && n.fract() == 0.0 => {
                let id = NodeId::from_u64(*n as u64);
                if self.doc.contains(id) {
                    Ok(id)
                } else {
                    Err("node is not part of this document".to_string())
                }
            }
            _ => Err("expected a node".to_string()),
        }
    }

    fn optional_node(&self, value: Option<&Reply>) -> Result<Option<NodeId>, String> {
        match value {
            Some(Reply::Null) | Some(Reply::Undefined) | None => Ok(None),
            Some(Reply::Number(n)) if *n < 0.0 => Ok(None),
            other => self.node(other).map(Some),
        }
    }

    fn create(&mut self, data: NodeData) -> Result<NodeId, String> {
        if self.added >= MAX_ADDED_NODES {
            return Err("document node limit reached".to_string());
        }
        self.added += 1;
        Ok(self.doc.create_node(data))
    }

    /// Runs primitive `op`. Errors become a JavaScript exception.
    pub fn call(&mut self, op: &str, args: &[Reply]) -> Result<Reply, String> {
        let text = |i: usize| -> String {
            match args.get(i) {
                Some(Reply::Text(s)) => s.clone(),
                Some(Reply::Number(n)) => n.to_string(),
                Some(Reply::Bool(b)) => b.to_string(),
                _ => String::new(),
            }
        };
        Ok(match op {
            // ---- WebGL ----
            "glCreate" => {
                if self.gl.len() >= 16 {
                    return Err("too many WebGL contexts".into());
                }
                let canvas = self.node(args.first())?;
                let num = |i: usize| match args.get(i) {
                    Some(Reply::Number(n)) => *n,
                    _ => 0.0,
                };
                let (w, h) = (num(1).clamp(1.0, 8192.0) as u32, num(2).clamp(1.0, 8192.0) as u32);
                let mut ctx = blueice_gl::Context::new(w, h);
                ctx.has_alpha = !matches!(args.get(3), Some(Reply::Bool(false)));
                ctx.premultiplied_alpha = !matches!(args.get(4), Some(Reply::Bool(false)));
                let id = self.next_gl;
                self.next_gl += 1;
                self.gl.insert(id, crate::web::gl_bridge::GlContext { ctx, canvas, dirty: true });
                Reply::Number(f64::from(id))
            }
            "gl" => {
                let id = match args.first() {
                    Some(Reply::Number(n)) => *n as u32,
                    _ => return Err("a GL call needs a context".into()),
                };
                let name = text(1);
                let g = self.gl.get_mut(&id).ok_or("no such GL context")?;
                crate::web::gl_bridge::call(g, &name, &args[2..])?
            }
            // The RGBA pixels of an element's picture, as `WxH:base64`.
            "imagePixels" => {
                let id = self.node(args.first())?;
                match self.images.get(&id) {
                    Some(img) if img.width * img.height <= 16_000_000 => Reply::Text(format!(
                        "{}x{}:{}",
                        img.width,
                        img.height,
                        crate::web::net::base64_encode(&img.rgba)
                    )),
                    _ => Reply::Null,
                }
            }
            "glPng" => {
                let id = match args.first() {
                    Some(Reply::Number(n)) => *n as u32,
                    _ => return Err("a GL call needs a context".into()),
                };
                let g = self.gl.get(&id).ok_or("no such GL context")?;
                let image = blueice_image::RasterImage::new(g.ctx.width, g.ctx.height, g.ctx.picture());
                match blueice_image::encode_png(&image) {
                    Some(png) => Reply::Text(crate::web::net::base64_encode(&png)),
                    None => Reply::Null,
                }
            }
            // ---- identity and shape ----
            "root" => self.doc.root().into(),
            "documentElement" => self.child_element(self.doc.root(), "html").into(),
            "head" | "body" => {
                let html = self.child_element(self.doc.root(), "html");
                html.and_then(|h| self.child_element(h, op)).into()
            }
            "kind" => {
                let id = self.node(args.first())?;
                Reply::Number(match self.doc.data(id) {
                    NodeData::Element { tag_name, .. } if tag_name == "#comment" => 8.0,
                    NodeData::Element { tag_name, .. } if tag_name == "#fragment" => 11.0,
                    NodeData::Element { .. } => 1.0,
                    NodeData::Text { .. } => 3.0,
                    NodeData::Document => 9.0,
                })
            }
            "tag" => {
                let id = self.node(args.first())?;
                match self.doc.data(id) {
                    NodeData::Element { tag_name, .. } => tag_name.as_str().into(),
                    _ => Reply::Null,
                }
            }
            "parent" => {
                let id = self.node(args.first())?;
                self.doc.parent(id).into()
            }
            "firstChild" => {
                let id = self.node(args.first())?;
                self.doc.first_child(id).into()
            }
            "lastChild" => {
                let id = self.node(args.first())?;
                self.doc.last_child(id).into()
            }
            "nextSibling" => {
                let id = self.node(args.first())?;
                self.doc.next_sibling(id).into()
            }
            "prevSibling" => {
                let id = self.node(args.first())?;
                self.doc.prev_sibling(id).into()
            }
            "children" => {
                let id = self.node(args.first())?;
                ids_json(self.doc.children(id))
            }
            "connected" => {
                let mut id = self.node(args.first())?;
                while let Some(parent) = self.doc.parent(id) {
                    id = parent;
                }
                (id == self.doc.root()).into()
            }

            // ---- creation ----
            "createElement" => {
                let tag = text(0).to_ascii_lowercase();
                if tag.is_empty()
                    || tag.contains(|c: char| c.is_whitespace() || c == '<' || c == '>')
                {
                    return Err("InvalidCharacterError: invalid tag name".to_string());
                }
                self.create(NodeData::Element {
                    tag_name: tag,
                    attributes: Vec::new(),
                })?
                .into()
            }
            "createText" => self.create(NodeData::Text { data: text(0) })?.into(),
            "createComment" => self
                .create(NodeData::Element {
                    tag_name: "#comment".to_string(),
                    attributes: vec![("data".to_string(), text(0))],
                })?
                .into(),
            "createFragment" => self
                .create(NodeData::Element {
                    tag_name: "#fragment".to_string(),
                    attributes: Vec::new(),
                })?
                .into(),
            "clone" => {
                let id = self.node(args.first())?;
                let deep = matches!(args.get(1), Some(Reply::Bool(true)));
                self.clone_node(id, deep)?.into()
            }

            // ---- tree edits ----
            "append" | "insertBefore" => {
                let parent = self.node(args.first())?;
                let child = self.node(args.get(1))?;
                let reference = self.optional_node(args.get(2))?;
                self.insert(parent, child, reference)?;
                Reply::Undefined
            }
            "remove" => {
                let id = self.node(args.first())?;
                if self.doc.parent(id).is_some() {
                    self.doc.detach(id);
                    self.dirty = true;
                }
                Reply::Undefined
            }
            "replaceChildren" => {
                let parent = self.node(args.first())?;
                let kids: Vec<NodeId> = self.doc.children(parent).collect();
                for kid in kids {
                    self.doc.detach(kid);
                }
                self.dirty = true;
                Reply::Undefined
            }

            // ---- text ----
            "data" => {
                let id = self.node(args.first())?;
                match self.doc.data(id) {
                    NodeData::Text { data } => data.as_str().into(),
                    NodeData::Element {
                        attributes,
                        tag_name,
                    } if tag_name == "#comment" => attributes
                        .iter()
                        .find(|(k, _)| k == "data")
                        .map_or("", |(_, v)| v.as_str())
                        .into(),
                    _ => Reply::Null,
                }
            }
            "setData" => {
                let id = self.node(args.first())?;
                match self.doc.data_mut(id) {
                    NodeData::Text { data } => *data = text(1),
                    NodeData::Element {
                        attributes,
                        tag_name,
                    } if tag_name == "#comment" => set_attribute(attributes, "data", &text(1)),
                    _ => return Err("not a character data node".to_string()),
                }
                self.dirty = true;
                Reply::Undefined
            }
            "textContent" => {
                let id = self.node(args.first())?;
                let mut out = String::new();
                self.collect_text(id, &mut out);
                out.into()
            }
            "setTextContent" => {
                let id = self.node(args.first())?;
                let value = text(1);
                let kids: Vec<NodeId> = self.doc.children(id).collect();
                for kid in kids {
                    self.doc.detach(kid);
                }
                if !value.is_empty() {
                    let node = self.create(NodeData::Text { data: value })?;
                    self.doc.append_child(id, node);
                }
                self.dirty = true;
                Reply::Undefined
            }

            // ---- attributes ----
            "getAttr" => {
                let id = self.node(args.first())?;
                match self.doc.data(id) {
                    NodeData::Element { attributes, .. } => attributes
                        .iter()
                        .find(|(k, _)| *k == text(1))
                        .map_or(Reply::Null, |(_, v)| v.as_str().into()),
                    _ => Reply::Null,
                }
            }
            "setAttr" => {
                let id = self.node(args.first())?;
                if let NodeData::Element { attributes, .. } = self.doc.data_mut(id) {
                    set_attribute(attributes, &text(1), &text(2));
                    self.dirty = true;
                }
                Reply::Undefined
            }
            "removeAttr" => {
                let id = self.node(args.first())?;
                if let NodeData::Element { attributes, .. } = self.doc.data_mut(id) {
                    let name = text(1);
                    let before = attributes.len();
                    attributes.retain(|(k, _)| *k != name);
                    if attributes.len() != before {
                        self.dirty = true;
                    }
                }
                Reply::Undefined
            }
            "attrs" => {
                let id = self.node(args.first())?;
                match self.doc.data(id) {
                    NodeData::Element { attributes, .. } => {
                        let mut out = String::from("[");
                        for (i, (k, v)) in attributes.iter().enumerate() {
                            if i > 0 {
                                out.push(',');
                            }
                            out.push('[');
                            json_string(k, &mut out);
                            out.push(',');
                            json_string(v, &mut out);
                            out.push(']');
                        }
                        out.push(']');
                        out.into()
                    }
                    _ => "[]".into(),
                }
            }

            // ---- selectors and lookup ----
            "query" | "queryAll" | "matches" => {
                let root = self.node(args.first())?;
                let selectors = blueice_css::parse_selectors(&text(1));
                if selectors.is_empty() && !text(1).trim().is_empty() {
                    return Err(format!(
                        "SyntaxError: '{}' is not a valid selector",
                        text(1)
                    ));
                }
                let hit = |doc: &Document, id: NodeId| {
                    selectors.iter().any(|s| blueice_css::matches(doc, id, s))
                };
                match op {
                    "matches" => hit(&self.doc, root).into(),
                    "query" => self
                        .descendants(root)
                        .into_iter()
                        .find(|&id| hit(&self.doc, id))
                        .into(),
                    _ => ids_json(
                        self.descendants(root)
                            .into_iter()
                            .filter(|&id| hit(&self.doc, id)),
                    ),
                }
            }
            "byId" => {
                let wanted = text(0);
                self.descendants(self.doc.root())
                    .into_iter()
                    .find(|&id| {
                        matches!(self.doc.data(id), NodeData::Element { attributes, .. }
                            if attributes.iter().any(|(k, v)| k == "id" && *v == wanted))
                    })
                    .into()
            }

            // ---- markup ----
            "innerHTML" => {
                let id = self.node(args.first())?;
                let mut out = String::new();
                for child in self.doc.children(id) {
                    self.serialize(child, &mut out);
                }
                out.into()
            }
            "outerHTML" => {
                let id = self.node(args.first())?;
                let mut out = String::new();
                self.serialize(id, &mut out);
                out.into()
            }
            "setInnerHTML" => {
                let id = self.node(args.first())?;
                let kids: Vec<NodeId> = self.doc.children(id).collect();
                for kid in kids {
                    self.doc.detach(kid);
                }
                let context = match self.doc.data(id) {
                    NodeData::Element { tag_name, .. } => tag_name.clone(),
                    _ => "div".to_string(),
                };
                for node in self.parse_fragment(&text(1), &context)? {
                    self.doc.append_child(id, node);
                }
                self.dirty = true;
                Reply::Undefined
            }
            "parseFragment" => {
                let fragment = self.create(NodeData::Element {
                    tag_name: "#fragment".to_string(),
                    attributes: Vec::new(),
                })?;
                for node in self.parse_fragment(&text(0), &text(1))? {
                    self.doc.append_child(fragment, node);
                }
                fragment.into()
            }

            "parseDocument" => {
                let parsed = blueice_html::parse(&text(0));
                let fragment = self.create(NodeData::Element {
                    tag_name: "#fragment".to_string(),
                    attributes: Vec::new(),
                })?;
                let html = parsed.children(parsed.root()).find(|&c| {
                    matches!(parsed.data(c), NodeData::Element { tag_name, .. } if tag_name == "html")
                });
                if let Some(html) = html {
                    let copy = self.copy_from(&parsed, html)?;
                    self.doc.append_child(fragment, copy);
                }
                fragment.into()
            }

            // ---- style and geometry ----
            "computed" => {
                let id = self.node(args.first())?;
                self.computed_value(id, &text(1))
                    .map_or(Reply::Text(String::new()), Reply::Text)
            }
            "rect" => {
                let id = self.node(args.first())?;
                match self.rects.get(&id) {
                    Some((x, y, w, h)) => format!("[{x},{y},{w},{h}]").into(),
                    None => "[0,0,0,0]".into(),
                }
            }
            "viewport" => format!("[{},{}]", self.viewport.0, self.viewport.1).into(),
            "media" => {
                let env = blueice_css::MediaEnvironment {
                    width: self.viewport.0,
                    height: self.viewport.1,
                    ..Default::default()
                };
                blueice_css::matches_media(&text(0), &env).into()
            }

            // ---- host requests ----
            "navigate" => {
                self.navigations.push(NavigationRequest::Load {
                    url: text(0),
                    replace: matches!(args.get(1), Some(Reply::Bool(true))),
                });
                Reply::Undefined
            }
            "pushState" => {
                self.navigations.push(NavigationRequest::PushState {
                    url: text(0),
                    replace: matches!(args.get(1), Some(Reply::Bool(true))),
                });
                self.url = text(0);
                Reply::Undefined
            }
            "reload" => {
                self.navigations.push(NavigationRequest::Reload);
                Reply::Undefined
            }
            "go" => {
                let delta = match args.first() {
                    Some(Reply::Number(n)) => *n as i32,
                    _ => 0,
                };
                self.navigations.push(NavigationRequest::Go(delta));
                Reply::Undefined
            }
            "loadScript" => {
                let id = self.node(args.first())?;
                self.script_requests.push((id, text(1)));
                Reply::Undefined
            }
            "netStart" => {
                // (method, url, headers as JSON pairs, body as base64)
                let headers: Vec<(String, String)> =
                    serde_json::from_str::<Vec<(String, String)>>(&text(2)).unwrap_or_default();
                let body = crate::web::net::base64_decode(&text(3)).unwrap_or_default();
                let origin = url::Url::parse(&self.url)
                    .map(|u| u.origin().ascii_serialization())
                    .unwrap_or_default();
                let id = self.next_request;
                self.next_request += 1;
                if self.net_jobs.len() >= 64 {
                    return Err("too many requests are pending".to_string());
                }
                self.net_jobs.push(crate::web::net::NetJob {
                    id,
                    method: text(0),
                    url: text(1),
                    headers,
                    body,
                    origin,
                });
                Reply::Number(id as f64)
            }
            "url" => self.url.as_str().into(),
            "cookie" => self.cookies.as_str().into(),
            "setCookie" => {
                self.set_cookie(&text(0));
                Reply::Undefined
            }
            "parseUrl" => parse_url(&text(0), &text(1)),
            other => return Err(format!("unknown DOM primitive '{other}'")),
        })
    }

    fn child_element(&self, parent: NodeId, tag: &str) -> Option<NodeId> {
        self.doc.children(parent).find(
            |&c| matches!(self.doc.data(c), NodeData::Element { tag_name, .. } if tag_name == tag),
        )
    }

    /// Every element below `root`, in document order.
    fn descendants(&self, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.doc.children(root).collect();
        stack.reverse();
        while let Some(id) = stack.pop() {
            if matches!(self.doc.data(id), NodeData::Element { .. }) {
                out.push(id);
            }
            let mut kids: Vec<NodeId> = self.doc.children(id).collect();
            kids.reverse();
            stack.extend(kids);
        }
        out
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        match self.doc.data(id) {
            NodeData::Text { data } => out.push_str(data),
            NodeData::Element { tag_name, .. } if tag_name == "#comment" => {}
            _ => {
                for child in self.doc.children(id) {
                    self.collect_text(child, out);
                }
            }
        }
    }

    fn insert(
        &mut self,
        parent: NodeId,
        child: NodeId,
        reference: Option<NodeId>,
    ) -> Result<(), String> {
        if child == parent || self.is_ancestor(child, parent) {
            return Err("HierarchyRequestError: a node cannot contain itself".to_string());
        }
        if let Some(reference) = reference {
            if self.doc.parent(reference) != Some(parent) {
                return Err("NotFoundError: the reference node is not a child".to_string());
            }
        }
        let is_fragment = matches!(self.doc.data(child),
            NodeData::Element { tag_name, .. } if tag_name == "#fragment");
        if is_fragment {
            let kids: Vec<NodeId> = self.doc.children(child).collect();
            for kid in kids {
                self.doc.detach(kid);
                self.doc.insert_before(parent, kid, reference);
            }
        } else {
            if self.doc.parent(child).is_some() {
                self.doc.detach(child);
            }
            // Inserting a node before itself keeps it where it is.
            let reference = reference.filter(|&r| r != child);
            self.doc.insert_before(parent, child, reference);
        }
        self.dirty = true;
        Ok(())
    }

    fn is_ancestor(&self, ancestor: NodeId, mut node: NodeId) -> bool {
        while let Some(parent) = self.doc.parent(node) {
            if parent == ancestor {
                return true;
            }
            node = parent;
        }
        false
    }

    fn clone_node(&mut self, id: NodeId, deep: bool) -> Result<NodeId, String> {
        let data = match self.doc.data(id) {
            NodeData::Document => return Err("a document cannot be cloned".to_string()),
            other => other.clone(),
        };
        let copy = self.create(data)?;
        if deep {
            let kids: Vec<NodeId> = self.doc.children(id).collect();
            for kid in kids {
                let kid_copy = self.clone_node(kid, true)?;
                self.doc.append_child(copy, kid_copy);
            }
        }
        Ok(copy)
    }

    /// Parses `html` as the children of an element named `context`, and
    /// returns the new (parentless) nodes.
    fn parse_fragment(&mut self, html: &str, context: &str) -> Result<Vec<NodeId>, String> {
        let (open, close) = match context {
            "table" | "thead" | "tbody" | "tfoot" => ("<table><tbody>", "</tbody></table>"),
            "tr" => ("<table><tbody><tr>", "</tr></tbody></table>"),
            "select" | "optgroup" => ("<select>", "</select>"),
            "colgroup" => ("<table><colgroup>", "</colgroup></table>"),
            _ => ("", ""),
        };
        let parsed = blueice_html::parse(&format!("<body>{open}{html}{close}"));
        let html_el = parsed
            .children(parsed.root())
            .find(|&c| matches!(parsed.data(c), NodeData::Element { tag_name, .. } if tag_name == "html"))
            .ok_or("fragment parse failed")?;
        let body = parsed
            .children(html_el)
            .find(|&c| matches!(parsed.data(c), NodeData::Element { tag_name, .. } if tag_name == "body"))
            .ok_or("fragment parse failed")?;
        let mut holder = body;
        // Unwrap the context scaffolding added above.
        for _ in 0..match context {
            "table" | "thead" | "tbody" | "tfoot" => 2,
            "tr" => 3,
            "select" | "optgroup" | "colgroup" => 1,
            _ => 0,
        } {
            holder = match parsed
                .children(holder)
                .find(|&c| matches!(parsed.data(c), NodeData::Element { .. }))
            {
                Some(next) => next,
                None => break,
            };
        }
        let mut out = Vec::new();
        let kids: Vec<NodeId> = parsed.children(holder).collect();
        for kid in kids {
            out.push(self.copy_from(&parsed, kid)?);
        }
        Ok(out)
    }

    fn copy_from(&mut self, from: &Document, id: NodeId) -> Result<NodeId, String> {
        let data = match from.data(id) {
            NodeData::Document => return Err("unexpected document node".to_string()),
            other => other.clone(),
        };
        let node = self.create(data)?;
        for child in from.children(id).collect::<Vec<_>>() {
            let copy = self.copy_from(from, child)?;
            self.doc.append_child(node, copy);
        }
        Ok(node)
    }

    fn serialize(&self, id: NodeId, out: &mut String) {
        match self.doc.data(id) {
            NodeData::Text { data } => {
                let raw = self.doc.parent(id).is_some_and(|p| {
                    matches!(self.doc.data(p), NodeData::Element { tag_name, .. }
                        if matches!(tag_name.as_str(), "script" | "style"))
                });
                if raw {
                    out.push_str(data);
                } else {
                    escape_text(data, out);
                }
            }
            NodeData::Element {
                tag_name,
                attributes,
            } if tag_name == "#comment" => {
                out.push_str("<!--");
                out.push_str(
                    attributes
                        .iter()
                        .find(|(k, _)| k == "data")
                        .map_or("", |(_, v)| v),
                );
                out.push_str("-->");
            }
            NodeData::Element { tag_name, .. } if tag_name == "#fragment" => {
                for child in self.doc.children(id) {
                    self.serialize(child, out);
                }
            }
            NodeData::Element {
                tag_name,
                attributes,
            } => {
                out.push('<');
                out.push_str(tag_name);
                for (k, v) in attributes {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    escape_attribute(v, out);
                    out.push('"');
                }
                out.push('>');
                if !VOID.contains(&tag_name.as_str()) {
                    for child in self.doc.children(id) {
                        self.serialize(child, out);
                    }
                    out.push_str("</");
                    out.push_str(tag_name);
                    out.push('>');
                }
            }
            NodeData::Document => {
                for child in self.doc.children(id) {
                    self.serialize(child, out);
                }
            }
        }
    }

    fn computed_value(&self, id: NodeId, property: &str) -> Option<String> {
        let style = self.styles.get(&id)?;
        let property = property.trim().to_ascii_lowercase();
        match property.as_str() {
            "display" => Some(style.display.clone()),
            "color" => Some(css_text(&Value::Color(style.color))),
            "font-size" => Some(format!("{}px", style.font_size_px)),
            "text-align" => style.text_align.clone(),
            "font-family" => style.font_family.clone(),
            "font-style" => style.font_style.clone().or(Some("normal".into())),
            other => style.other.get(other).map(css_text),
        }
    }

    /// `document.cookie = "name=value; attributes"`.
    fn set_cookie(&mut self, assignment: &str) {
        let pair = assignment.split(';').next().unwrap_or("").trim();
        let Some((name, value)) = pair.split_once('=') else {
            return;
        };
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let expired = assignment.split(';').skip(1).any(|a| {
            let a = a.trim().to_ascii_lowercase();
            a.starts_with("max-age=0")
                || a.starts_with("max-age=-")
                || a.starts_with("expires=thu, 01 jan 1970")
        });
        let mut jar: Vec<(String, String)> = self
            .cookies
            .split("; ")
            .filter(|p| !p.is_empty())
            .filter_map(|p| {
                p.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .filter(|(k, _)| k != name)
            .collect();
        if !expired {
            jar.push((name.to_string(), value.trim().to_string()));
        }
        self.cookies = jar
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
    }
}

const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

fn set_attribute(attributes: &mut Vec<(String, String)>, name: &str, value: &str) {
    match attributes.iter_mut().find(|(k, _)| k == name) {
        Some((_, v)) => *v = value.to_string(),
        None => attributes.push((name.to_string(), value.to_string())),
    }
}

fn ids_json(ids: impl Iterator<Item = NodeId>) -> Reply {
    let mut out = String::from("[");
    for (i, id) in ids.enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&id.as_u64().to_string());
    }
    out.push(']');
    Reply::Text(out)
}

fn json_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            c => out.push(c),
        }
    }
}

fn escape_attribute(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            c => out.push(c),
        }
    }
}

/// A computed value as CSS text.
fn css_text(value: &Value) -> String {
    use blueice_css::Color;
    match value {
        Value::Color(Color::Rgba(r, g, b, 255)) => format!("rgb({r}, {g}, {b})"),
        Value::Color(Color::Rgba(r, g, b, a)) => {
            format!("rgba({r}, {g}, {b}, {})", f64::from(*a) / 255.0)
        }
        Value::Color(Color::CurrentColor) => "currentcolor".to_string(),
        Value::Keyword(k) => k.clone(),
        Value::Number(n) => n.to_string(),
        Value::Percentage(p) => format!("{p}%"),
        Value::Length(l) => format!(
            "{}px",
            l.to_px(&blueice_css::LengthContext {
                font_size: 16.0,
                root_font_size: 16.0,
                viewport_width: 1280.0,
                viewport_height: 720.0,
                percent_base: f64::NAN,
            })
        ),
        Value::List(parts) => parts.iter().map(css_text).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }
}

/// `new URL(input, base)`: the parts as a JSON object, or `null`.
fn parse_url(input: &str, base: &str) -> Reply {
    let parsed = if base.is_empty() {
        url::Url::parse(input)
    } else {
        url::Url::parse(base).and_then(|b| b.join(input))
    };
    let Ok(u) = parsed else {
        return Reply::Null;
    };
    let mut out = String::from("{");
    let field = |name: &str, value: &str, first: bool, out: &mut String| {
        if !first {
            out.push(',');
        }
        json_string(name, out);
        out.push(':');
        json_string(value, out);
    };
    let host = u.host_str().unwrap_or("");
    let port = u.port().map(|p| p.to_string()).unwrap_or_default();
    let origin = match u.scheme() {
        "http" | "https" | "ws" | "wss" | "ftp" => u.origin().ascii_serialization(),
        _ => "null".to_string(),
    };
    field("href", u.as_str(), true, &mut out);
    field("protocol", &format!("{}:", u.scheme()), false, &mut out);
    field(
        "host",
        &if port.is_empty() {
            host.to_string()
        } else {
            format!("{host}:{port}")
        },
        false,
        &mut out,
    );
    field("hostname", host, false, &mut out);
    field("port", &port, false, &mut out);
    field("pathname", u.path(), false, &mut out);
    field(
        "search",
        &u.query().map_or(String::new(), |q| format!("?{q}")),
        false,
        &mut out,
    );
    field(
        "hash",
        &u.fragment().map_or(String::new(), |f| format!("#{f}")),
        false,
        &mut out,
    );
    field("origin", &origin, false, &mut out);
    field("username", u.username(), false, &mut out);
    field("password", u.password().unwrap_or(""), false, &mut out);
    out.push('}');
    Reply::Text(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(html: &str) -> DomState {
        DomState::new(
            blueice_html::parse(html),
            "https://example.com/a/b?x=1#h",
            (1280.0, 720.0),
        )
    }

    fn n(id: impl Into<Reply>) -> Reply {
        id.into()
    }

    fn id_of(reply: Reply) -> NodeId {
        match reply {
            Reply::Number(v) => NodeId::from_u64(v as u64),
            other => panic!("expected a node, got {other:?}"),
        }
    }

    fn text(s: &str) -> Reply {
        Reply::Text(s.to_string())
    }

    #[test]
    fn the_document_has_its_standard_parts() {
        let mut s = state("<p id=a>hi</p>");
        let html = id_of(s.call("documentElement", &[]).unwrap());
        let body = id_of(s.call("body", &[]).unwrap());
        let head = id_of(s.call("head", &[]).unwrap());
        assert_eq!(s.call("tag", &[n(html)]).unwrap(), text("html"));
        assert_eq!(s.doc.parent(body), Some(html));
        assert_eq!(s.doc.parent(head), Some(html));
        let a = id_of(s.call("byId", &[text("a")]).unwrap());
        assert_eq!(s.call("tag", &[n(a)]).unwrap(), text("p"));
        assert_eq!(s.call("byId", &[text("none")]).unwrap(), Reply::Null);
    }

    #[test]
    fn creating_inserting_and_removing_nodes_moves_them_and_marks_dirty() {
        let mut s = state("<ul><li>a</li></ul>");
        let ul = id_of(s.call("query", &[n(s.doc.root()), text("ul")]).unwrap());
        let li = id_of(s.call("createElement", &[text("LI")]).unwrap());
        assert_eq!(
            s.call("tag", &[n(li)]).unwrap(),
            text("li"),
            "tag names are lowercased"
        );
        s.dirty = false;
        s.call("append", &[n(ul), n(li), Reply::Null]).unwrap();
        assert!(s.dirty);
        assert_eq!(s.doc.children(ul).count(), 2);
        let first = s.doc.first_child(ul).unwrap();
        s.call("insertBefore", &[n(ul), n(li), n(first)]).unwrap();
        assert_eq!(
            s.doc.first_child(ul),
            Some(li),
            "moved before the first child"
        );
        assert_eq!(s.doc.children(ul).count(), 2);
        s.call("remove", &[n(li)]).unwrap();
        assert_eq!(s.doc.children(ul).count(), 1);
        assert_eq!(s.call("connected", &[n(li)]).unwrap(), Reply::Bool(false));
    }

    #[test]
    fn a_node_cannot_contain_itself_and_a_stale_reference_is_refused() {
        let mut s = state("<div><p></p></div>");
        let div = id_of(s.call("query", &[n(s.doc.root()), text("div")]).unwrap());
        let p = id_of(s.call("query", &[n(s.doc.root()), text("p")]).unwrap());
        let err = s.call("append", &[n(p), n(div), Reply::Null]).unwrap_err();
        assert!(err.starts_with("HierarchyRequestError"));
        let other = id_of(s.call("createElement", &[text("b")]).unwrap());
        let err = s.call("insertBefore", &[n(div), n(other), n(other)]);
        assert!(err.is_err());
        assert!(s.call("tag", &[Reply::Number(9.0e9)]).is_err());
    }

    #[test]
    fn a_fragment_hands_over_its_children() {
        let mut s = state("<div></div>");
        let div = id_of(s.call("query", &[n(s.doc.root()), text("div")]).unwrap());
        let fragment = id_of(s.call("createFragment", &[]).unwrap());
        for tag in ["a", "b"] {
            let el = id_of(s.call("createElement", &[text(tag)]).unwrap());
            s.call("append", &[n(fragment), n(el), Reply::Null])
                .unwrap();
        }
        s.call("append", &[n(div), n(fragment), Reply::Null])
            .unwrap();
        assert_eq!(s.doc.children(div).count(), 2);
        assert_eq!(s.doc.children(fragment).count(), 0);
    }

    #[test]
    fn attributes_read_write_and_list() {
        let mut s = state("<a id=x href=/q>t</a>");
        let a = id_of(s.call("byId", &[text("x")]).unwrap());
        assert_eq!(
            s.call("getAttr", &[n(a), text("href")]).unwrap(),
            text("/q")
        );
        assert_eq!(
            s.call("getAttr", &[n(a), text("nope")]).unwrap(),
            Reply::Null
        );
        s.call("setAttr", &[n(a), text("class"), text("k \"q\"")])
            .unwrap();
        s.call("removeAttr", &[n(a), text("href")]).unwrap();
        assert_eq!(
            s.call("attrs", &[n(a)]).unwrap(),
            text(r#"[["id","x"],["class","k \"q\""]]"#)
        );
    }

    #[test]
    fn text_content_reads_descendants_and_replaces_children() {
        let mut s = state("<p id=p>a<b>b</b>c</p>");
        let p = id_of(s.call("byId", &[text("p")]).unwrap());
        assert_eq!(s.call("textContent", &[n(p)]).unwrap(), text("abc"));
        s.call("setTextContent", &[n(p), text("x<y")]).unwrap();
        assert_eq!(s.call("textContent", &[n(p)]).unwrap(), text("x<y"));
        assert_eq!(s.doc.children(p).count(), 1);
        assert_eq!(s.call("innerHTML", &[n(p)]).unwrap(), text("x&lt;y"));
    }

    #[test]
    fn selectors_find_in_document_order_and_match_single_nodes() {
        let mut s = state(
            "<div class=a><p class=b id=1></p><p class=b id=2></p></div><p class=b id=3></p>",
        );
        let root = s.doc.root();
        let found = s
            .call("queryAll", &[n(root), text("div p.b, p#3")])
            .unwrap();
        let Reply::Text(json) = found else { panic!() };
        assert_eq!(json.matches(',').count(), 2, "three matches: {json}");
        let first = id_of(s.call("query", &[n(root), text("p.b")]).unwrap());
        assert_eq!(
            s.call("getAttr", &[n(first), text("id")]).unwrap(),
            text("1")
        );
        assert_eq!(
            s.call("matches", &[n(first), text("div > p")]).unwrap(),
            Reply::Bool(true)
        );
        assert_eq!(
            s.call("matches", &[n(first), text("section p")]).unwrap(),
            Reply::Bool(false)
        );
        let err = s.call("query", &[n(root), text("p[")]).unwrap_err();
        assert!(err.starts_with("SyntaxError"));
    }

    #[test]
    fn inner_html_round_trips_and_replaces_children() {
        let mut s = state("<div id=d><i>old</i></div>");
        let d = id_of(s.call("byId", &[text("d")]).unwrap());
        s.call(
            "setInnerHTML",
            &[
                n(d),
                text("<b class=\"x\">1</b> & <br><img src=\"a&quot;b\">"),
            ],
        )
        .unwrap();
        assert_eq!(
            s.call("innerHTML", &[n(d)]).unwrap(),
            text("<b class=\"x\">1</b> &amp; <br><img src=\"a&quot;b\">")
        );
        assert_eq!(
            s.call("outerHTML", &[n(d)]).unwrap(),
            text("<div id=\"d\"><b class=\"x\">1</b> &amp; <br><img src=\"a&quot;b\"></div>")
        );
    }

    #[test]
    fn table_rows_parse_in_a_table_context() {
        let mut s = state("<table id=t><tbody></tbody></table>");
        let t = id_of(s.call("byId", &[text("t")]).unwrap());
        let tbody = id_of(s.call("query", &[n(t), text("tbody")]).unwrap());
        s.call(
            "setInnerHTML",
            &[n(tbody), text("<tr><td>1</td></tr><tr><td>2</td></tr>")],
        )
        .unwrap();
        assert_eq!(s.doc.children(tbody).count(), 2);
    }

    #[test]
    fn script_and_style_text_serializes_raw() {
        let mut s = state("<div id=d></div>");
        let d = id_of(s.call("byId", &[text("d")]).unwrap());
        s.call(
            "setInnerHTML",
            &[n(d), text("<script>if (a<b && c) {}</script>")],
        )
        .unwrap();
        assert_eq!(
            s.call("innerHTML", &[n(d)]).unwrap(),
            text("<script>if (a<b && c) {}</script>")
        );
    }

    #[test]
    fn cloning_is_deep_or_shallow_and_leaves_the_original() {
        let mut s = state("<div id=d class=k><p>x</p></div>");
        let d = id_of(s.call("byId", &[text("d")]).unwrap());
        let shallow = id_of(s.call("clone", &[n(d), Reply::Bool(false)]).unwrap());
        let deep = id_of(s.call("clone", &[n(d), Reply::Bool(true)]).unwrap());
        assert_eq!(s.doc.children(shallow).count(), 0);
        assert_eq!(s.doc.children(deep).count(), 1);
        assert_ne!(deep, d);
        assert_eq!(
            s.call("getAttr", &[n(deep), text("class")]).unwrap(),
            text("k")
        );
        assert_eq!(s.call("parent", &[n(deep)]).unwrap(), Reply::Null);
    }

    #[test]
    fn comments_are_hidden_nodes_with_data() {
        let mut s = state("<div id=d>a</div>");
        let d = id_of(s.call("byId", &[text("d")]).unwrap());
        let c = id_of(s.call("createComment", &[text("note")]).unwrap());
        s.call("append", &[n(d), n(c), Reply::Null]).unwrap();
        assert_eq!(s.call("kind", &[n(c)]).unwrap(), Reply::Number(8.0));
        assert_eq!(s.call("data", &[n(c)]).unwrap(), text("note"));
        assert_eq!(
            s.call("textContent", &[n(d)]).unwrap(),
            text("a"),
            "comments add no text"
        );
        assert_eq!(s.call("innerHTML", &[n(d)]).unwrap(), text("a<!--note-->"));
    }

    #[test]
    fn a_whole_document_parses_into_a_detached_fragment() {
        let mut s = state("");
        let fragment = id_of(
            s.call(
                "parseDocument",
                &[text(
                    "<!doctype html><title>T</title><body><p id=x>hi</p></body>",
                )],
            )
            .unwrap(),
        );
        let html = id_of(s.call("query", &[n(fragment), text("html")]).unwrap());
        assert_eq!(s.call("tag", &[n(html)]).unwrap(), text("html"));
        let p = id_of(s.call("query", &[n(fragment), text("body p")]).unwrap());
        assert_eq!(s.call("connected", &[n(p)]).unwrap(), Reply::Bool(false));
        assert_eq!(
            s.call("byId", &[text("x")]).unwrap(),
            Reply::Null,
            "not in the live document"
        );
    }

    #[test]
    fn urls_parse_with_a_base_and_reject_nonsense() {
        let mut s = state("");
        let Reply::Text(json) = s
            .call(
                "parseUrl",
                &[text("../q?x=1#f"), text("https://h.example:8080/a/b/c")],
            )
            .unwrap()
        else {
            panic!()
        };
        assert!(
            json.contains("\"href\":\"https://h.example:8080/a/q?x=1#f\""),
            "{json}"
        );
        assert!(json.contains("\"host\":\"h.example:8080\""));
        assert!(json.contains("\"origin\":\"https://h.example:8080\""));
        assert_eq!(
            s.call("parseUrl", &[text("nope"), text("")]).unwrap(),
            Reply::Null
        );
    }

    #[test]
    fn cookies_set_replace_and_expire() {
        let mut s = state("");
        s.call("setCookie", &[text("a=1; path=/")]).unwrap();
        s.call("setCookie", &[text("b=2")]).unwrap();
        s.call("setCookie", &[text("a=3")]).unwrap();
        assert_eq!(s.call("cookie", &[]).unwrap(), text("b=2; a=3"));
        s.call("setCookie", &[text("b=; max-age=0")]).unwrap();
        assert_eq!(s.call("cookie", &[]).unwrap(), text("a=3"));
    }

    #[test]
    fn navigation_requests_are_recorded_for_the_host() {
        let mut s = state("");
        s.call(
            "pushState",
            &[text("https://example.com/next"), Reply::Bool(false)],
        )
        .unwrap();
        s.call(
            "navigate",
            &[text("https://example.com/other"), Reply::Bool(true)],
        )
        .unwrap();
        s.call("go", &[Reply::Number(-1.0)]).unwrap();
        assert_eq!(s.url, "https://example.com/next");
        assert_eq!(
            s.navigations,
            vec![
                NavigationRequest::PushState {
                    url: "https://example.com/next".into(),
                    replace: false
                },
                NavigationRequest::Load {
                    url: "https://example.com/other".into(),
                    replace: true
                },
                NavigationRequest::Go(-1),
            ]
        );
    }

    #[test]
    fn media_queries_follow_the_viewport() {
        let mut s = state("");
        assert_eq!(
            s.call("media", &[text("(min-width: 1000px)")]).unwrap(),
            Reply::Bool(true)
        );
        assert_eq!(
            s.call("media", &[text("(max-width: 600px)")]).unwrap(),
            Reply::Bool(false)
        );
    }

    #[test]
    fn unknown_primitives_and_the_node_cap_are_errors() {
        let mut s = state("");
        assert!(s.call("frobnicate", &[]).is_err());
        s.added = MAX_ADDED_NODES;
        assert!(s.call("createText", &[text("x")]).is_err());
    }
}
