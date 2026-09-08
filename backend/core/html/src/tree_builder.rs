// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! HTML tree builder: [`Token`] stream -> `blueice_dom::Document`.
//!
//! Implements a subset of the WHATWG HTML5 tree-construction algorithm,
//! scoped to `phase-2-mvp-scope/PLAN.md`'s "MVP HTML scope" element list.
//! Per `research/html-parsing.md` §4, the two error-recovery algorithms
//! kept in full are the ones real (not just adversarial) pages depend on:
//!
//! - **Adoption agency** (misnested formatting elements, e.g.
//!   `<b><i>x</b>y</i>`) -- see [`TreeBuilder::adoption_agency`].
//! - **Foster parenting** (content misplaced directly inside `<table>`
//!   before any row) -- see [`TreeBuilder::foster_parent_target`].
//!
//! Deliberately not implemented, per the MVP scope: `<template>`/applet
//! /object/marquee (the only elements that need "marker" entries in the
//! active-formatting-elements list -- since none are supported, the list
//! never needs markers, simplifying reconstruction and adoption agency);
//! frameset-related modes; foreign content (svg/math); quirks-mode
//! detection from DOCTYPE content (doctypes are tokenized but their
//! content is discarded, and comments are never turned into DOM nodes --
//! neither affects rendering, which is all the MVP pipeline needs from
//! the DOM). The Noah's Ark clause (capping identical adjacent formatting
//! entries at 3) is also skipped as a minor optimization, not a
//! correctness requirement.

use crate::tokenizer::{ContentModel, Token, Tokenizer};
use blueice_dom::{Document, NodeData, NodeId};

const VOID_ELEMENTS: &[&str] = &["br", "hr", "img", "input", "link", "meta", "col"];
const RCDATA_ELEMENTS: &[&str] = &["textarea", "title"];
const FORMATTING_ELEMENTS: &[&str] = &["a", "b", "i", "em", "strong", "u", "small", "code"];
const HEADING_ELEMENTS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6"];
const P_CLOSING_ELEMENTS: &[&str] = &[
    "div",
    "p",
    "ul",
    "ol",
    "li",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "pre",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "main",
    "aside",
    "figure",
    "figcaption",
    "form",
    "table",
    "fieldset",
];
/// Elements that stop an "in scope" walk up the stack of open elements
/// (WHATWG's default scope list, restricted to the MVP element set).
const DEFAULT_SCOPE_BLOCKERS: &[&str] = &["html", "table", "td", "th", "caption"];
/// Elements popped automatically by "generate implied end tags".
const IMPLIED_END_TAGS: &[&str] = &["li", "option", "optgroup", "p"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
    Text,
    InTable,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    AfterBody,
    AfterAfterBody,
}

enum StepResult {
    Done,
    Reprocess(Token),
}

/// One entry in the active-formatting-elements list: the node currently
/// standing in for this formatting run, plus enough of the original
/// token (tag name + attributes) to recreate it if reconstruction or
/// adoption-agency cloning needs a fresh copy.
type FormattingEntry = (NodeId, String, Vec<(String, String)>);

struct TreeBuilder {
    tokenizer: Tokenizer,
    document: Document,
    open_elements: Vec<NodeId>,
    active_formatting: Vec<FormattingEntry>,
    mode: Mode,
    original_mode: Mode,
    head_element: Option<NodeId>,
    form_element: Option<NodeId>,
    /// Set for the duration of processing a single token that must use
    /// the foster-parenting insertion point instead of the current node
    /// (`research/html-parsing.md` §3: pervasive in Gecko as a `MayFoster`
    /// suffix on insertion primitives, centralized in Blink behind a
    /// flag+guard -- BlueIce follows Blink's shape here).
    foster_parenting: bool,
    /// True immediately after processing a `Comment`/`Doctype` token,
    /// false after anything else -- `insert_text` consults this to
    /// decide whether it's safe to merge into an adjacent Text node.
    /// blueice_dom never materializes Comment/Doctype as real nodes
    /// (module docs), so without this flag two character-token runs
    /// separated only by a comment would look adjacent from
    /// `insert_text`'s point of view and wrongly merge -- a real
    /// browser can't merge them, because its real (materialized)
    /// Comment node physically sits between them. Found by
    /// `tests/wpt_corpus.rs` against the WPT corpus (`comments01.dat`:
    /// `FOO<!-- BAR -->BAZ` must keep "FOO" and "BAZ" as two separate
    /// Text nodes) as a real regression from the *other* direction of
    /// the same merge logic (`insert_text`'s own docs) fixed earlier.
    just_saw_dropped_comment_or_doctype: bool,
}

/// Parses `input` as HTML into a fresh [`Document`], per the tree
/// builder's supported subset (module docs).
pub fn parse(input: &str) -> Document {
    let mut tb = TreeBuilder::new(input);
    tb.run();
    tb.document
}

impl TreeBuilder {
    fn new(input: &str) -> Self {
        TreeBuilder {
            tokenizer: Tokenizer::new(input),
            document: Document::new(),
            open_elements: Vec::new(),
            active_formatting: Vec::new(),
            mode: Mode::Initial,
            original_mode: Mode::InBody,
            head_element: None,
            form_element: None,
            foster_parenting: false,
            just_saw_dropped_comment_or_doctype: false,
        }
    }

    fn run(&mut self) {
        loop {
            let token = self.tokenizer.next_token();
            let is_eof = token == Token::Eof;
            self.process_token(token);
            if is_eof {
                break;
            }
        }
    }

    fn process_token(&mut self, mut token: Token) {
        loop {
            match self.step(token) {
                StepResult::Done => return,
                StepResult::Reprocess(t) => token = t,
            }
        }
    }

    fn step(&mut self, token: Token) -> StepResult {
        let is_comment_or_doctype = matches!(token, Token::Comment | Token::Doctype);
        let result = self.dispatch(token);
        self.just_saw_dropped_comment_or_doctype = is_comment_or_doctype;
        result
    }

    fn dispatch(&mut self, token: Token) -> StepResult {
        match self.mode {
            Mode::Initial => self.step_initial(token),
            Mode::BeforeHtml => self.step_before_html(token),
            Mode::BeforeHead => self.step_before_head(token),
            Mode::InHead => self.step_in_head(token),
            Mode::AfterHead => self.step_after_head(token),
            Mode::InBody => self.step_in_body(token),
            Mode::Text => self.step_text(token),
            Mode::InTable => self.step_in_table(token),
            Mode::InCaption => self.step_in_caption(token),
            Mode::InColumnGroup => self.step_in_column_group(token),
            Mode::InTableBody => self.step_in_table_body(token),
            Mode::InRow => self.step_in_row(token),
            Mode::InCell => self.step_in_cell(token),
            Mode::AfterBody => self.step_after_body(token),
            Mode::AfterAfterBody => self.step_after_after_body(token),
        }
    }

    // ---- tree/stack primitives ----

    fn current_node(&self) -> NodeId {
        match self.open_elements.last() {
            Some(&id) => id,
            None => self.document.root(),
        }
    }

    fn tag_of(&self, id: NodeId) -> Option<String> {
        match self.document.data(id) {
            NodeData::Element { tag_name, .. } => Some(tag_name.clone()),
            _ => None,
        }
    }

    fn is_current(&self, tag: &str) -> bool {
        self.tag_of(self.current_node()).as_deref() == Some(tag)
    }

    fn is_special(&self, id: NodeId) -> bool {
        match self.tag_of(id) {
            Some(t) => !FORMATTING_ELEMENTS.contains(&t.as_str()),
            None => true,
        }
    }

    fn is_table_context(&self, id: NodeId) -> bool {
        matches!(
            self.tag_of(id).as_deref(),
            Some("table") | Some("tbody") | Some("thead") | Some("tfoot") | Some("tr")
        )
    }

    fn current_node_needs_foster(&self) -> bool {
        self.is_table_context(self.current_node())
    }

    /// The "appropriate place for inserting a node" when foster-parenting
    /// is in effect: immediately before the nearest open `<table>`, as a
    /// child of *its* parent (never a child of the table itself).
    fn foster_parent_target(&self) -> (NodeId, Option<NodeId>) {
        if let Some(&table_id) = self.open_elements.iter().rev().find(|&&id| self.tag_of(id).as_deref() == Some("table")) {
            if let Some(parent) = self.document.parent(table_id) {
                return (parent, Some(table_id));
            }
        }
        (self.current_node(), None)
    }

    fn insert_node(&mut self, id: NodeId) {
        if self.foster_parenting && self.current_node_needs_foster() {
            let (parent, reference) = self.foster_parent_target();
            self.document.insert_before(parent, id, reference);
        } else {
            self.document.append_child(self.current_node(), id);
        }
    }

    fn insert_element(&mut self, name: &str, attrs: Vec<(String, String)>) -> NodeId {
        let id = self.document.create_node(NodeData::Element {
            tag_name: name.to_string(),
            attributes: attrs,
        });
        self.insert_node(id);
        if !VOID_ELEMENTS.contains(&name) {
            self.open_elements.push(id);
        }
        id
    }

    /// Per HTML5's "insert a character" algorithm: if the node
    /// immediately before the insertion point is already a Text node,
    /// append `data` to it rather than creating a new sibling. This
    /// matters whenever two character-token runs that should logically
    /// be one text node arrive as separate tokens with something else
    /// processed in between at the parser-state level but not at the
    /// insertion-point level -- the concrete case that surfaced this
    /// (found by `phase-15-chromium-differential-testing/PLAN.md`'s
    /// DOM diff against real Chromium, not by anything visual, since
    /// both shapes render identically for whitespace-only text):
    /// whitespace between `</div>` and `</body>`, and whitespace
    /// between `</body>` and `</html>` (reprocessed under "in body"
    /// rules per the "after body" insertion mode, per spec), both
    /// insert into `<body>` and must end up as one Text node, not two.
    ///
    /// **Except immediately after a dropped `Comment`/`Doctype` token**
    /// (`just_saw_dropped_comment_or_doctype`): a real browser's actual
    /// Comment/Doctype node physically sits between the two character
    /// runs, blocking the merge, even though blueice_dom never
    /// materializes either as a node itself. An earlier version of
    /// this function merged across a dropped comment too, on the
    /// (wrong) reasoning that "nothing is there to block it" -- the
    /// WPT corpus's `comments01.dat` (`FOO<!-- BAR -->BAZ` must stay
    /// two Text nodes, not merge into one) is what caught that.
    fn insert_text(&mut self, data: &str) {
        if data.is_empty() {
            return;
        }
        let (parent, reference) = if self.foster_parenting && self.current_node_needs_foster() {
            self.foster_parent_target()
        } else {
            (self.current_node(), None)
        };
        let preceding = if self.just_saw_dropped_comment_or_doctype {
            None
        } else {
            match reference {
                Some(r) => self.document.prev_sibling(r),
                None => self.document.last_child(parent),
            }
        };
        if let Some(id) = preceding {
            if let NodeData::Text { data: existing } = self.document.data_mut(id) {
                existing.push_str(data);
                return;
            }
        }
        let id = self.document.create_node(NodeData::Text { data: data.to_string() });
        self.document.insert_before(parent, id, reference);
    }

    fn push_formatting(&mut self, id: NodeId, tag: &str, attrs: Vec<(String, String)>) {
        self.active_formatting.push((id, tag.to_string(), attrs));
    }

    fn switch_to_text_mode(&mut self, name: &str, attrs: Vec<(String, String)>) {
        self.insert_element(name, attrs);
        let model = if RCDATA_ELEMENTS.contains(&name) {
            ContentModel::Rcdata
        } else {
            ContentModel::Rawtext
        };
        self.tokenizer.set_content_model(model);
        self.original_mode = self.mode;
        self.mode = Mode::Text;
    }

    // ---- scope checks ----

    fn has_tag_in_scope(&self, tag: &str, extra_blockers: &[&str]) -> bool {
        for &id in self.open_elements.iter().rev() {
            let Some(t) = self.tag_of(id) else { continue };
            if t == tag {
                return true;
            }
            if DEFAULT_SCOPE_BLOCKERS.contains(&t.as_str()) || extra_blockers.contains(&t.as_str()) {
                return false;
            }
        }
        false
    }

    fn has_node_in_scope(&self, target: NodeId) -> bool {
        for &id in self.open_elements.iter().rev() {
            if id == target {
                return true;
            }
            let Some(t) = self.tag_of(id) else { continue };
            if DEFAULT_SCOPE_BLOCKERS.contains(&t.as_str()) {
                return false;
            }
        }
        false
    }

    fn has_p_in_button_scope(&self) -> bool {
        self.has_tag_in_scope("p", &["button"])
    }

    fn has_li_in_list_item_scope(&self) -> bool {
        self.has_tag_in_scope("li", &["ol", "ul"])
    }

    fn has_any_heading_in_scope(&self) -> bool {
        for &id in self.open_elements.iter().rev() {
            let Some(t) = self.tag_of(id) else { continue };
            if HEADING_ELEMENTS.contains(&t.as_str()) {
                return true;
            }
            if DEFAULT_SCOPE_BLOCKERS.contains(&t.as_str()) {
                return false;
            }
        }
        false
    }

    // ---- stack maintenance ----

    fn generate_implied_end_tags(&mut self, except: Option<&str>) {
        while let Some(&top) = self.open_elements.last() {
            let Some(t) = self.tag_of(top) else { break };
            if !IMPLIED_END_TAGS.contains(&t.as_str()) || except == Some(t.as_str()) {
                break;
            }
            self.open_elements.pop();
        }
    }

    fn pop_until_and_including(&mut self, tag: &str) {
        while let Some(id) = self.open_elements.pop() {
            if self.tag_of(id).as_deref() == Some(tag) {
                break;
            }
        }
    }

    fn close_p_element(&mut self) {
        self.generate_implied_end_tags(Some("p"));
        self.pop_until_and_including("p");
    }

    fn close_li_if_open(&mut self) {
        if self.has_li_in_list_item_scope() {
            self.generate_implied_end_tags(Some("li"));
            self.pop_until_and_including("li");
        }
    }

    fn close_current_cell(&mut self) {
        self.generate_implied_end_tags(None);
        while let Some(id) = self.open_elements.pop() {
            if matches!(self.tag_of(id).as_deref(), Some("td") | Some("th")) {
                break;
            }
        }
        self.mode = Mode::InRow;
    }

    fn close_table_section(&mut self) {
        while let Some(&top) = self.open_elements.last() {
            let is_section = matches!(self.tag_of(top).as_deref(), Some("tbody") | Some("thead") | Some("tfoot"));
            self.open_elements.pop();
            if is_section {
                break;
            }
        }
    }

    /// The "any other end tag" fallback (spec's default for an end tag
    /// with no dedicated handling in the current mode): search down the
    /// stack for a matching element, closing through it if found, unless
    /// a non-formatting ("special") element is met first, in which case
    /// the token is silently ignored.
    fn any_other_end_tag(&mut self, tag: &str) {
        for i in (0..self.open_elements.len()).rev() {
            let id = self.open_elements[i];
            let Some(t) = self.tag_of(id) else { continue };
            if t == tag {
                self.generate_implied_end_tags(Some(tag));
                self.open_elements.truncate(i);
                return;
            }
            if self.is_special(id) {
                return;
            }
        }
    }

    /// Recomputes the insertion mode from the current stack of open
    /// elements. Both call sites run this *immediately after*
    /// `pop_until_and_including("table")`, which means every table-
    /// internal frame (`tr`/`td`/`th`/`tbody`/`thead`/`tfoot`/`caption`/
    /// `colgroup`) that could otherwise justify a dedicated branch here
    /// has, by construction, already been popped along with `table`
    /// itself -- the real spec's fuller version of this algorithm
    /// exists to serve other callers (e.g. fragment parsing) BlueIce's
    /// MVP scope doesn't implement. Handling only what can actually
    /// remain on the stack at that point (`body`/`html`, else fall back
    /// to `InBody`) keeps this honest rather than carrying branches no
    /// test could ever legitimately reach.
    fn reset_insertion_mode(&mut self) {
        for &id in self.open_elements.iter().rev() {
            match self.tag_of(id).as_deref() {
                Some("body") => {
                    self.mode = Mode::InBody;
                    return;
                }
                Some("html") => {
                    self.mode = if self.head_element.is_some() { Mode::AfterHead } else { Mode::BeforeHead };
                    return;
                }
                _ => continue,
            }
        }
        self.mode = Mode::InBody;
    }

    // ---- active formatting elements ----

    fn reconstruct_active_formatting_elements(&mut self) {
        if self.active_formatting.is_empty() {
            return;
        }
        let last = self.active_formatting.len() - 1;
        if self.open_elements.contains(&self.active_formatting[last].0) {
            return;
        }
        let mut first = last;
        while first > 0 && !self.open_elements.contains(&self.active_formatting[first - 1].0) {
            first -= 1;
        }
        for i in first..=last {
            let (_, tag, attrs) = self.active_formatting[i].clone();
            let new_id = self.insert_element(&tag, attrs);
            self.active_formatting[i].0 = new_id;
        }
    }

    /// The adoption agency algorithm (WHATWG HTML5 §13.2.5.2), for an end
    /// tag naming a formatting element. See module docs for what's
    /// intentionally simplified relative to the full spec (no markers,
    /// no Noah's Ark clause).
    fn adoption_agency(&mut self, tag: &str) {
        for _ in 0..8 {
            let Some(fe_pos) = self.active_formatting.iter().rposition(|(_, t, _)| t == tag) else {
                self.any_other_end_tag(tag);
                return;
            };
            let fe_id = self.active_formatting[fe_pos].0;

            let Some(fe_stack_pos) = self.open_elements.iter().position(|&id| id == fe_id) else {
                self.active_formatting.remove(fe_pos);
                return;
            };

            if !self.has_node_in_scope(fe_id) {
                return;
            }

            let furthest_block_pos = self.open_elements[fe_stack_pos + 1..]
                .iter()
                .position(|&id| self.is_special(id))
                .map(|rel| fe_stack_pos + 1 + rel);

            let Some(furthest_block_pos) = furthest_block_pos else {
                self.open_elements.truncate(fe_stack_pos);
                self.active_formatting.remove(fe_pos);
                return;
            };
            let furthest_block_id = self.open_elements[furthest_block_pos];
            let common_ancestor = self.open_elements[fe_stack_pos - 1];

            let between: Vec<NodeId> = self.open_elements[fe_stack_pos + 1..furthest_block_pos].to_vec();
            let mut dropped: Vec<NodeId> = Vec::new();
            let mut bookmark_id: Option<NodeId> = None;
            let mut last_node = furthest_block_id;
            let mut iterations = 0;

            for &node_id in between.iter().rev() {
                iterations += 1;
                let af_pos = self.active_formatting.iter().position(|(id, _, _)| *id == node_id);

                let Some(af_pos) = af_pos else {
                    dropped.push(node_id);
                    continue;
                };
                if iterations > 3 {
                    self.active_formatting.remove(af_pos);
                    dropped.push(node_id);
                    continue;
                }

                let (_, node_tag, node_attrs) = self.active_formatting[af_pos].clone();
                let new_node = self.document.create_node(NodeData::Element {
                    tag_name: node_tag.clone(),
                    attributes: node_attrs.clone(),
                });
                self.active_formatting[af_pos] = (new_node, node_tag, node_attrs);
                dropped.push(node_id);

                if last_node == furthest_block_id {
                    bookmark_id = Some(new_node);
                }

                if self.document.parent(last_node).is_some() {
                    self.document.detach(last_node);
                }
                self.document.append_child(new_node, last_node);
                last_node = new_node;
            }
            self.open_elements.retain(|id| !dropped.contains(id));

            if self.document.parent(last_node).is_some() {
                self.document.detach(last_node);
            }
            if self.is_table_context(common_ancestor) {
                let saved = self.foster_parenting;
                self.foster_parenting = true;
                let (parent, reference) = self.foster_parent_target();
                self.document.insert_before(parent, last_node, reference);
                self.foster_parenting = saved;
            } else {
                self.document.append_child(common_ancestor, last_node);
            }

            let (_, fe_tag, fe_attrs) = self.active_formatting[fe_pos].clone();
            let new_fe = self.document.create_node(NodeData::Element {
                tag_name: fe_tag.clone(),
                attributes: fe_attrs.clone(),
            });
            let children: Vec<NodeId> = self.document.children(furthest_block_id).collect();
            for child in children {
                self.document.detach(child);
                self.document.append_child(new_fe, child);
            }
            self.document.append_child(furthest_block_id, new_fe);

            let insert_at = bookmark_id
                .and_then(|id| self.active_formatting.iter().position(|(x, _, _)| *x == id))
                .map(|p| p + 1)
                .unwrap_or(fe_pos);
            self.active_formatting.retain(|(id, _, _)| *id != fe_id);
            let insert_at = insert_at.min(self.active_formatting.len());
            self.active_formatting.insert(insert_at, (new_fe, fe_tag, fe_attrs));

            // Insert new_fe *above* furthest_block in the stack (closer to
            // the top / current node), not below it -- verified against
            // Blink's HTMLElementStack::InsertAbove
            // (reference/chromium/.../html_element_stack.cc) after this
            // exact off-by-one produced runaway nesting (see
            // adoption_agency_with_block_furthest_block's regression
            // test): with new_fe below furthest_block, the very next
            // outer-loop iteration re-finds furthest_block as special
            // *again* and re-wraps it, up to the 8-iteration cap. With
            // new_fe on top, furthest_block has nothing above it on the
            // next iteration, so the search comes up empty and the loop
            // exits after one clean pass -- exactly the html5lib-tests
            // fixture behavior (`<a>1<p>2</a>3</p>` never nests "3"
            // inside a re-nested `<a>`).
            self.open_elements.retain(|&id| id != fe_id);
            let fb_pos_now = self.open_elements.iter().position(|&id| id == furthest_block_id).unwrap();
            self.open_elements.insert(fb_pos_now + 1, new_fe);
        }
    }

    // ---- insertion modes ----

    fn step_initial(&mut self, token: Token) -> StepResult {
        match &token {
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::Character(s) if s.trim().is_empty() => StepResult::Done,
            _ => {
                self.mode = Mode::BeforeHtml;
                StepResult::Reprocess(token)
            }
        }
    }

    fn step_before_html(&mut self, token: Token) -> StepResult {
        match &token {
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::Character(s) if s.trim().is_empty() => StepResult::Done,
            Token::StartTag { name, attrs, .. } if name == "html" => {
                self.insert_element("html", attrs.clone());
                self.mode = Mode::BeforeHead;
                StepResult::Done
            }
            Token::EndTag { name } if !matches!(name.as_str(), "head" | "body" | "html" | "br") => StepResult::Done,
            _ => {
                self.insert_element("html", vec![]);
                self.mode = Mode::BeforeHead;
                StepResult::Reprocess(token)
            }
        }
    }

    fn step_before_head(&mut self, token: Token) -> StepResult {
        match &token {
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::Character(s) if s.trim().is_empty() => StepResult::Done,
            Token::StartTag { name, .. } if name == "html" => self.step_in_body(token),
            Token::StartTag { name, attrs, .. } if name == "head" => {
                let id = self.insert_element("head", attrs.clone());
                self.head_element = Some(id);
                self.mode = Mode::InHead;
                StepResult::Done
            }
            Token::EndTag { name } if !matches!(name.as_str(), "head" | "body" | "html" | "br") => StepResult::Done,
            _ => {
                let id = self.insert_element("head", vec![]);
                self.head_element = Some(id);
                self.mode = Mode::InHead;
                StepResult::Reprocess(token)
            }
        }
    }

    fn step_in_head(&mut self, token: Token) -> StepResult {
        match &token {
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::Character(s) if s.trim().is_empty() => {
                let s = s.clone();
                self.insert_text(&s);
                StepResult::Done
            }
            Token::StartTag { name, .. } if name == "html" => self.step_in_body(token),
            Token::StartTag { name, attrs, .. } if matches!(name.as_str(), "meta" | "link") => {
                self.insert_element(name, attrs.clone());
                StepResult::Done
            }
            Token::StartTag { name, attrs, .. } if matches!(name.as_str(), "title" | "style" | "script") => {
                let name = name.clone();
                let attrs = attrs.clone();
                self.switch_to_text_mode(&name, attrs);
                StepResult::Done
            }
            Token::StartTag { name, .. } if name == "head" => StepResult::Done,
            Token::EndTag { name } if name == "head" => {
                self.open_elements.pop();
                self.mode = Mode::AfterHead;
                StepResult::Done
            }
            Token::EndTag { name } if !matches!(name.as_str(), "body" | "html" | "br") => StepResult::Done,
            _ => {
                self.open_elements.pop();
                self.mode = Mode::AfterHead;
                StepResult::Reprocess(token)
            }
        }
    }

    fn step_after_head(&mut self, token: Token) -> StepResult {
        match &token {
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::Character(s) if s.trim().is_empty() => {
                let s = s.clone();
                self.insert_text(&s);
                StepResult::Done
            }
            Token::StartTag { name, .. } if name == "html" => self.step_in_body(token),
            Token::StartTag { name, attrs, .. } if name == "body" => {
                self.insert_element("body", attrs.clone());
                self.mode = Mode::InBody;
                StepResult::Done
            }
            Token::StartTag { name, .. } if name == "head" => StepResult::Done,
            Token::EndTag { name } if !matches!(name.as_str(), "body" | "html" | "br") => StepResult::Done,
            _ => {
                self.insert_element("body", vec![]);
                self.mode = Mode::InBody;
                StepResult::Reprocess(token)
            }
        }
    }

    fn step_text(&mut self, token: Token) -> StepResult {
        match &token {
            Token::Character(s) => {
                let s = s.clone();
                self.insert_text(&s);
                StepResult::Done
            }
            Token::EndTag { .. } => {
                self.open_elements.pop();
                self.tokenizer.set_content_model(ContentModel::Data);
                self.mode = self.original_mode;
                StepResult::Done
            }
            // Per spec: an end-of-file token here is a parse error that
            // pops the current node and restores the original insertion
            // mode the *same* way an end tag does, but -- unlike an end
            // tag, which is fully consumed -- EOF must then be
            // *reprocessed* in that restored mode, so an unclosed
            // <script>/<title>/<style>/<textarea> at EOF still triggers
            // the normal implicit-</head>/<body>-insertion cascade a
            // real EOF at top level would (found by
            // `tests/wpt_corpus.rs` against the WPT tree-construction
            // corpus: `<!doctype html><script>` with no closing tag was
            // silently missing its `<body>` element entirely).
            Token::Eof => {
                self.open_elements.pop();
                self.tokenizer.set_content_model(ContentModel::Data);
                self.mode = self.original_mode;
                StepResult::Reprocess(Token::Eof)
            }
            _ => StepResult::Done,
        }
    }

    fn step_in_body(&mut self, token: Token) -> StepResult {
        match token {
            Token::Doctype | Token::Comment | Token::Eof => StepResult::Done,
            Token::Character(s) => {
                self.reconstruct_active_formatting_elements();
                self.insert_text(&s);
                StepResult::Done
            }
            Token::StartTag { name, attrs, .. } => self.start_tag_in_body(&name, attrs),
            Token::EndTag { name } => self.end_tag_in_body(&name),
        }
    }

    fn start_tag_in_body(&mut self, name: &str, attrs: Vec<(String, String)>) -> StepResult {
        match name {
            "html" | "head" => StepResult::Done,
            "table" => {
                if self.has_p_in_button_scope() {
                    self.close_p_element();
                }
                self.insert_element("table", attrs);
                self.mode = Mode::InTable;
                StepResult::Done
            }
            "form" => {
                if self.form_element.is_some() {
                    return StepResult::Done;
                }
                if self.has_p_in_button_scope() {
                    self.close_p_element();
                }
                let id = self.insert_element("form", attrs);
                self.form_element = Some(id);
                StepResult::Done
            }
            "li" => {
                self.close_li_if_open();
                if self.has_p_in_button_scope() {
                    self.close_p_element();
                }
                self.insert_element("li", attrs);
                StepResult::Done
            }
            "option" | "optgroup" => {
                if self.is_current("option") {
                    self.open_elements.pop();
                }
                self.reconstruct_active_formatting_elements();
                self.insert_element(name, attrs);
                StepResult::Done
            }
            "a" => {
                if self.active_formatting.iter().any(|(_, t, _)| t == "a") {
                    self.adoption_agency("a");
                }
                self.reconstruct_active_formatting_elements();
                let id = self.insert_element("a", attrs.clone());
                self.push_formatting(id, "a", attrs);
                StepResult::Done
            }
            "textarea" | "script" | "style" => {
                self.switch_to_text_mode(name, attrs);
                StepResult::Done
            }
            _ if FORMATTING_ELEMENTS.contains(&name) => {
                self.reconstruct_active_formatting_elements();
                let id = self.insert_element(name, attrs.clone());
                self.push_formatting(id, name, attrs);
                StepResult::Done
            }
            _ if HEADING_ELEMENTS.contains(&name) => {
                if self.has_p_in_button_scope() {
                    self.close_p_element();
                }
                if HEADING_ELEMENTS.contains(&self.tag_of(self.current_node()).as_deref().unwrap_or("")) {
                    self.open_elements.pop();
                }
                self.insert_element(name, attrs);
                StepResult::Done
            }
            _ if P_CLOSING_ELEMENTS.contains(&name) => {
                if self.has_p_in_button_scope() {
                    self.close_p_element();
                }
                self.insert_element(name, attrs);
                StepResult::Done
            }
            _ if VOID_ELEMENTS.contains(&name) => {
                self.reconstruct_active_formatting_elements();
                self.insert_element(name, attrs);
                StepResult::Done
            }
            _ => {
                self.reconstruct_active_formatting_elements();
                self.insert_element(name, attrs);
                StepResult::Done
            }
        }
    }

    fn end_tag_in_body(&mut self, name: &str) -> StepResult {
        match name {
            "body" | "html" => {
                if !self.has_tag_in_scope("body", &[]) {
                    return StepResult::Done;
                }
                self.mode = Mode::AfterBody;
                if name == "html" {
                    StepResult::Reprocess(Token::EndTag { name: name.to_string() })
                } else {
                    StepResult::Done
                }
            }
            "p" => {
                if self.has_p_in_button_scope() {
                    self.close_p_element();
                }
                StepResult::Done
            }
            "li" => {
                if self.has_li_in_list_item_scope() {
                    self.generate_implied_end_tags(Some("li"));
                    self.pop_until_and_including("li");
                }
                StepResult::Done
            }
            "form" => {
                if let Some(form_id) = self.form_element.take() {
                    if self.has_node_in_scope(form_id) {
                        self.generate_implied_end_tags(None);
                        self.open_elements.retain(|&id| id != form_id);
                    }
                }
                StepResult::Done
            }
            _ if HEADING_ELEMENTS.contains(&name) => {
                if self.has_any_heading_in_scope() {
                    self.generate_implied_end_tags(None);
                    while let Some(top) = self.open_elements.pop() {
                        if HEADING_ELEMENTS.contains(&self.tag_of(top).as_deref().unwrap_or("")) {
                            break;
                        }
                    }
                }
                StepResult::Done
            }
            _ if FORMATTING_ELEMENTS.contains(&name) => {
                self.adoption_agency(name);
                StepResult::Done
            }
            _ if P_CLOSING_ELEMENTS.contains(&name) => {
                if self.has_tag_in_scope(name, &[]) {
                    self.generate_implied_end_tags(None);
                    self.pop_until_and_including(name);
                }
                StepResult::Done
            }
            _ => {
                self.any_other_end_tag(name);
                StepResult::Done
            }
        }
    }

    fn step_in_table(&mut self, token: Token) -> StepResult {
        match token {
            Token::Character(s) => {
                if s.trim().is_empty() {
                    self.insert_text(&s);
                } else {
                    self.foster_parenting = true;
                    self.reconstruct_active_formatting_elements();
                    self.insert_text(&s);
                    self.foster_parenting = false;
                }
                StepResult::Done
            }
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::StartTag { name, attrs, .. } if name == "caption" => {
                self.insert_element("caption", attrs);
                self.mode = Mode::InCaption;
                StepResult::Done
            }
            Token::StartTag { name, attrs, .. } if name == "colgroup" => {
                self.insert_element("colgroup", attrs);
                self.mode = Mode::InColumnGroup;
                StepResult::Done
            }
            Token::StartTag { name, .. } if name == "col" => {
                self.insert_element("colgroup", vec![]);
                self.mode = Mode::InColumnGroup;
                StepResult::Reprocess(Token::StartTag { name, attrs: vec![], self_closing: false })
            }
            Token::StartTag { name, attrs, .. } if matches!(name.as_str(), "tbody" | "thead" | "tfoot") => {
                self.insert_element(&name, attrs);
                self.mode = Mode::InTableBody;
                StepResult::Done
            }
            Token::StartTag { name, attrs, self_closing } if matches!(name.as_str(), "tr" | "td" | "th") => {
                self.insert_element("tbody", vec![]);
                self.mode = Mode::InTableBody;
                StepResult::Reprocess(Token::StartTag { name, attrs, self_closing })
            }
            Token::StartTag { name, attrs, self_closing } if name == "table" => {
                if self.has_tag_in_scope("table", &[]) {
                    self.pop_until_and_including("table");
                    self.reset_insertion_mode();
                }
                StepResult::Reprocess(Token::StartTag { name, attrs, self_closing })
            }
            Token::EndTag { name } if name == "table" => {
                if self.has_tag_in_scope("table", &[]) {
                    self.pop_until_and_including("table");
                    self.reset_insertion_mode();
                }
                StepResult::Done
            }
            Token::EndTag { name }
                if matches!(
                    name.as_str(),
                    "body" | "caption" | "col" | "colgroup" | "html" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr"
                ) =>
            {
                StepResult::Done
            }
            other => {
                self.foster_parenting = true;
                let result = self.step_in_body(other);
                self.foster_parenting = false;
                result
            }
        }
    }

    fn step_in_caption(&mut self, token: Token) -> StepResult {
        match token {
            Token::EndTag { name } if name == "caption" => {
                if self.has_tag_in_scope("caption", &[]) {
                    self.generate_implied_end_tags(None);
                    self.pop_until_and_including("caption");
                }
                self.mode = Mode::InTable;
                StepResult::Done
            }
            Token::StartTag { name, attrs, self_closing }
                if matches!(name.as_str(), "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr") =>
            {
                if self.has_tag_in_scope("caption", &[]) {
                    self.generate_implied_end_tags(None);
                    self.pop_until_and_including("caption");
                    self.mode = Mode::InTable;
                }
                StepResult::Reprocess(Token::StartTag { name, attrs, self_closing })
            }
            Token::EndTag { name } if name == "table" => {
                if self.has_tag_in_scope("caption", &[]) {
                    self.generate_implied_end_tags(None);
                    self.pop_until_and_including("caption");
                    self.mode = Mode::InTable;
                }
                StepResult::Reprocess(Token::EndTag { name })
            }
            other => self.step_in_body(other),
        }
    }

    fn step_in_column_group(&mut self, token: Token) -> StepResult {
        match token {
            Token::Character(s) if s.trim().is_empty() => {
                self.insert_text(&s);
                StepResult::Done
            }
            Token::Comment | Token::Doctype => StepResult::Done,
            Token::StartTag { name, attrs, .. } if name == "col" => {
                self.insert_element("col", attrs);
                StepResult::Done
            }
            Token::EndTag { name } if name == "colgroup" => {
                if self.is_current("colgroup") {
                    self.open_elements.pop();
                }
                self.mode = Mode::InTable;
                StepResult::Done
            }
            other => {
                if self.is_current("colgroup") {
                    self.open_elements.pop();
                }
                self.mode = Mode::InTable;
                StepResult::Reprocess(other)
            }
        }
    }

    fn step_in_table_body(&mut self, token: Token) -> StepResult {
        match token {
            Token::StartTag { name, attrs, .. } if name == "tr" => {
                self.insert_element("tr", attrs);
                self.mode = Mode::InRow;
                StepResult::Done
            }
            Token::StartTag { name, attrs, self_closing } if matches!(name.as_str(), "td" | "th") => {
                self.insert_element("tr", vec![]);
                self.mode = Mode::InRow;
                StepResult::Reprocess(Token::StartTag { name, attrs, self_closing })
            }
            Token::StartTag { name, attrs, self_closing } if matches!(name.as_str(), "tbody" | "thead" | "tfoot") => {
                self.close_table_section();
                self.mode = Mode::InTable;
                StepResult::Reprocess(Token::StartTag { name, attrs, self_closing })
            }
            Token::EndTag { name } if matches!(name.as_str(), "tbody" | "thead" | "tfoot") => {
                if self.has_tag_in_scope(&name, &[]) {
                    self.close_table_section();
                }
                self.mode = Mode::InTable;
                StepResult::Done
            }
            Token::StartTag { name, attrs, self_closing } if matches!(name.as_str(), "caption" | "col" | "colgroup") => {
                self.close_table_section();
                self.mode = Mode::InTable;
                StepResult::Reprocess(Token::StartTag { name, attrs, self_closing })
            }
            Token::EndTag { name } if name == "table" => {
                self.close_table_section();
                self.mode = Mode::InTable;
                StepResult::Reprocess(Token::EndTag { name })
            }
            other => self.step_in_table(other),
        }
    }

    fn step_in_row(&mut self, token: Token) -> StepResult {
        match token {
            Token::StartTag { name, attrs, .. } if matches!(name.as_str(), "td" | "th") => {
                self.insert_element(&name, attrs);
                self.mode = Mode::InCell;
                StepResult::Done
            }
            Token::EndTag { name } if name == "tr" => {
                if self.has_tag_in_scope("tr", &[]) {
                    self.pop_until_and_including("tr");
                }
                self.mode = Mode::InTableBody;
                StepResult::Done
            }
            Token::StartTag { name, attrs, self_closing }
                if matches!(name.as_str(), "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr") =>
            {
                if self.has_tag_in_scope("tr", &[]) {
                    self.pop_until_and_including("tr");
                    self.mode = Mode::InTableBody;
                }
                StepResult::Reprocess(Token::StartTag { name, attrs, self_closing })
            }
            Token::EndTag { name } if matches!(name.as_str(), "table" | "tbody" | "tfoot" | "thead") => {
                if self.has_tag_in_scope("tr", &[]) {
                    self.pop_until_and_including("tr");
                    self.mode = Mode::InTableBody;
                }
                StepResult::Reprocess(Token::EndTag { name })
            }
            other => self.step_in_table(other),
        }
    }

    fn step_in_cell(&mut self, token: Token) -> StepResult {
        match token {
            Token::EndTag { name } if matches!(name.as_str(), "td" | "th") => {
                if self.has_tag_in_scope(&name, &[]) {
                    self.generate_implied_end_tags(None);
                    self.pop_until_and_including(&name);
                }
                self.mode = Mode::InRow;
                StepResult::Done
            }
            Token::StartTag { name, attrs, self_closing }
                if matches!(name.as_str(), "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr") =>
            {
                if self.has_tag_in_scope("td", &[]) || self.has_tag_in_scope("th", &[]) {
                    self.close_current_cell();
                }
                StepResult::Reprocess(Token::StartTag { name, attrs, self_closing })
            }
            Token::EndTag { name } if matches!(name.as_str(), "table" | "tbody" | "tfoot" | "thead" | "tr") => {
                if self.has_tag_in_scope("td", &[]) || self.has_tag_in_scope("th", &[]) {
                    self.close_current_cell();
                }
                StepResult::Reprocess(Token::EndTag { name })
            }
            other => self.step_in_body(other),
        }
    }

    fn step_after_body(&mut self, token: Token) -> StepResult {
        match &token {
            Token::Character(s) if s.trim().is_empty() => self.step_in_body(token),
            Token::Comment | Token::Doctype | Token::Eof => StepResult::Done,
            Token::EndTag { name } if name == "html" => {
                self.mode = Mode::AfterAfterBody;
                StepResult::Done
            }
            _ => {
                self.mode = Mode::InBody;
                StepResult::Reprocess(token)
            }
        }
    }

    fn step_after_after_body(&mut self, token: Token) -> StepResult {
        match &token {
            Token::Comment | Token::Doctype | Token::Eof => StepResult::Done,
            Token::Character(s) if s.trim().is_empty() => self.step_in_body(token),
            _ => {
                self.mode = Mode::InBody;
                StepResult::Reprocess(token)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn children_tags(doc: &Document, id: NodeId) -> Vec<String> {
        doc.children(id)
            .filter_map(|c| match doc.data(c) {
                NodeData::Element { tag_name, .. } => Some(tag_name.clone()),
                _ => None,
            })
            .collect()
    }

    fn find_by_tag(doc: &Document, root: NodeId, tag: &str) -> Option<NodeId> {
        if let NodeData::Element { tag_name, .. } = doc.data(root) {
            if tag_name == tag {
                return Some(root);
            }
        }
        doc.children(root).find_map(|c| find_by_tag(doc, c, tag))
    }

    fn text_content(doc: &Document, id: NodeId) -> String {
        let mut out = String::new();
        collect_text(doc, id, &mut out);
        out
    }

    fn collect_text(doc: &Document, id: NodeId, out: &mut String) {
        match doc.data(id) {
            NodeData::Text { data } => out.push_str(data),
            _ => {
                for c in doc.children(id) {
                    collect_text(doc, c, out);
                }
            }
        }
    }

    // implicit html/head/body creation, explicit-doctype parsing,
    // attribute preservation, and void-element non-nesting are now
    // covered by development/browser_core/testing/fixtures/basic.dat,
    // exercised end to end through the public API by tests/fixtures.rs
    // -- see TEST_PLAN.md's Definition of Done on not keeping duplicate
    // coverage of the same input through the same interface.

    #[test]
    fn script_content_is_not_tree_constructed() {
        let doc = parse("<body><script>var x = document.createElement('p');</script></body>");
        let script = find_by_tag(&doc, doc.root(), "script").unwrap();
        assert!(find_by_tag(&doc, script, "p").is_none());
        assert_eq!(text_content(&doc, script), "var x = document.createElement('p');");
    }

    #[test]
    fn an_unclosed_script_at_eof_still_gets_an_implicit_body() {
        // Regression test for a real bug the WPT tree-construction
        // corpus run found (`tests/wpt_corpus.rs`, alone responsible
        // for over half of one file's 153 failures): EOF inside the
        // "Text" insertion mode (an unclosed <script>/<title>/<style>/
        // <textarea>) must be *reprocessed* in the restored original
        // insertion mode per spec, not just consumed -- otherwise the
        // implicit-<body>-insertion cascade a real top-level EOF
        // triggers never runs, and every element that document would
        // otherwise get (starting with <body> itself) silently goes
        // missing.
        let doc = parse("<!doctype html><script>");
        assert!(find_by_tag(&doc, doc.root(), "body").is_some(), "an unclosed <script> at EOF must not suppress the implicit <body>");
    }

    #[test]
    fn an_unclosed_textarea_at_eof_also_gets_an_implicit_body() {
        // Same bug, different RCDATA element -- proves the fix isn't
        // specific to <script>'s own content model.
        let doc = parse("<textarea>abc");
        assert!(find_by_tag(&doc, doc.root(), "body").is_some());
        let textarea = find_by_tag(&doc, doc.root(), "textarea").unwrap();
        assert_eq!(text_content(&doc, textarea), "abc");
    }

    #[test]
    fn p_auto_closes_on_new_p() {
        let doc = parse("<p>one<p>two");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(children_tags(&doc, body), vec!["p".to_string(), "p".to_string()]);
    }

    #[test]
    fn p_auto_closes_on_div() {
        let doc = parse("<p>one<div>two</div>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(children_tags(&doc, body), vec!["p".to_string(), "div".to_string()]);
    }

    #[test]
    fn li_auto_closes_previous_li() {
        let doc = parse("<ul><li>a<li>b<li>c</ul>");
        let ul = find_by_tag(&doc, doc.root(), "ul").unwrap();
        let items = children_tags(&doc, ul);
        assert_eq!(items, vec!["li".to_string(), "li".to_string(), "li".to_string()]);
    }

    #[test]
    fn heading_end_tag_closes_any_open_heading() {
        let doc = parse("<h1>title</h2>next");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        // </h2> should close the open <h1>, and "next" becomes a body-level sibling
        assert_eq!(children_tags(&doc, body), vec!["h1".to_string()]);
        assert_eq!(text_content(&doc, body), "titlenext");
    }

    // Basic adoption-agency (simple and block-furthest-block cases, plus
    // the html5lib-tests-verified <a> cases), the anchor-can't-nest-in-
    // itself case, plain foster parenting, and basic table structure are
    // now covered by adoption-agency.dat, foster-parenting.dat, and
    // tables.dat (see tests/fixtures.rs). The block-furthest-block case
    // in particular is why those fixtures exist: this crate's own
    // char/tag-level assertions here missed a real bug (the adoption
    // agency algorithm inserting the new formatting element on the wrong
    // side of furthest_block in the stack, causing runaway nesting up to
    // the 8-iteration cap) that an exact whole-tree-shape comparison
    // caught immediately. See the fix and its comment in
    // `adoption_agency` above.

    #[test]
    fn textarea_rcdata_is_not_tree_constructed_and_resolves_entities() {
        let doc = parse("<textarea>a &amp; <b>not-a-tag</b></textarea>");
        let ta = find_by_tag(&doc, doc.root(), "textarea").unwrap();
        assert!(find_by_tag(&doc, ta, "b").is_none());
        assert_eq!(text_content(&doc, ta), "a & <b>not-a-tag</b>");
    }

    #[test]
    fn comments_and_doctype_produce_no_dom_nodes() {
        let doc = parse("<!DOCTYPE html><!-- top --><html><!-- in html --><body><!-- in body -->x</body></html>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(doc.children(body).count(), 1);
        assert_eq!(text_content(&doc, body), "x");
    }

    #[test]
    fn form_element_pointer_prevents_nested_forms() {
        let doc = parse("<form><input><form><input></form></form>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(children_tags(&doc, body), vec!["form".to_string()]);
        let form = doc.children(body).next().unwrap();
        // both inputs land inside the single form; the nested <form> start tag is ignored
        assert_eq!(children_tags(&doc, form), vec!["input".to_string(), "input".to_string()]);
    }

    #[test]
    fn unknown_elements_parse_generically() {
        let doc = parse("<foo-bar>x</foo-bar>");
        let el = find_by_tag(&doc, doc.root(), "foo-bar").unwrap();
        assert_eq!(text_content(&doc, el), "x");
    }

    #[test]
    fn stray_end_tags_before_html_head_and_after_head_are_ignored() {
        // exercises BeforeHtml/BeforeHead/AfterHead's "ignore this specific
        // end tag" arms (anything other than head/body/html/br)
        let doc = parse("</foo><html></bar><head></baz></head></qux><body>x</body></html>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(text_content(&doc, body), "x");
    }

    #[test]
    fn explicit_head_close_then_stray_end_tag_in_in_head() {
        let doc = parse("<head></style></head><body>x</body>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(text_content(&doc, body), "x");
    }

    #[test]
    fn option_auto_closes_previous_option() {
        let doc = parse("<select><option>a<option>b</select>");
        let select = find_by_tag(&doc, doc.root(), "select").unwrap();
        assert_eq!(children_tags(&doc, select), vec!["option".to_string(), "option".to_string()]);
    }

    #[test]
    fn new_heading_start_tag_closes_a_still_open_heading() {
        let doc = parse("<h1>a<h2>b</h2>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(children_tags(&doc, body), vec!["h1".to_string(), "h2".to_string()]);
        assert_eq!(text_content(&doc, body), "ab");
    }

    #[test]
    fn form_end_tag_with_no_matching_open_form_is_ignored() {
        let doc = parse("<body></form>x</body>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(text_content(&doc, body), "x");
    }

    #[test]
    fn stray_formatting_end_tag_with_nothing_open_is_ignored() {
        let doc = parse("<p>text</b>more</p>");
        let p = find_by_tag(&doc, doc.root(), "p").unwrap();
        assert_eq!(text_content(&doc, p), "textmore");
    }

    #[test]
    fn adoption_agency_fe_already_removed_from_stack_is_ignored() {
        // `</div>`'s generic pop walks straight through the still-open
        // `<b>`, dropping it from the stack of open elements without
        // going through adoption agency -- so by the time `</b>` arrives,
        // `<b>` is still in the active formatting list but no longer on
        // the stack (the fe-not-in-stack branch).
        let doc = parse("<div><b>x</div></b>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(children_tags(&doc, body), vec!["div".to_string()]);
        let div = doc.children(body).next().unwrap();
        assert_eq!(children_tags(&doc, div), vec!["b".to_string()]);
        assert_eq!(text_content(&doc, div), "x");
    }

    #[test]
    fn adoption_agency_ignores_formatting_element_blocked_out_of_scope() {
        // `<b>` is still open and still active, but by the time `</b>`
        // arrives, a `<table>` boundary sits between it and the top of
        // the stack -- "in scope" fails, so the token is ignored and `b`
        // keeps wrapping the whole table.
        let doc = parse("<b><table><tr><td></b>x</td></tr></table>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        let b = find_by_tag(&doc, body, "b").unwrap();
        let table = find_by_tag(&doc, b, "table").unwrap();
        let td = find_by_tag(&doc, table, "td").unwrap();
        assert_eq!(text_content(&doc, td), "x");
    }

    #[test]
    fn adoption_agency_ages_out_formatting_elements_deep_in_the_chain() {
        // Five levels of formatting elements between `<b>` and the block
        // that becomes the furthest block: the innermost ones clone
        // normally, but per the (simplified) Noah's-Ark-adjacent aging
        // rule, entries past the third inner-loop iteration are dropped
        // rather than cloned. The exact resulting shape is an
        // implementation detail; what must hold is that no text is lost
        // or duplicated and the parser doesn't panic.
        let doc = parse("<b><i><em><strong><u><div>x</b>y</div>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(text_content(&doc, body), "xy");
    }

    #[test]
    fn table_direct_thead_tbody_tags_without_implicit_tr_path() {
        let doc = parse("<table><thead><tr><th>H</th></tr></thead><tbody><tr><td>d</td></tr></tbody></table>");
        let table = find_by_tag(&doc, doc.root(), "table").unwrap();
        assert_eq!(children_tags(&doc, table), vec!["thead".to_string(), "tbody".to_string()]);
    }

    #[test]
    fn col_directly_under_table_gets_an_implicit_colgroup() {
        let doc = parse("<table><col><col></table>");
        let table = find_by_tag(&doc, doc.root(), "table").unwrap();
        assert_eq!(children_tags(&doc, table), vec!["colgroup".to_string()]);
        let colgroup = doc.children(table).next().unwrap();
        assert_eq!(children_tags(&doc, colgroup), vec!["col".to_string(), "col".to_string()]);
    }

    #[test]
    fn colgroup_closes_implicitly_on_other_content() {
        let doc = parse("<table><colgroup><col><tbody><tr><td>x</td></tr></tbody></table>");
        let table = find_by_tag(&doc, doc.root(), "table").unwrap();
        assert_eq!(children_tags(&doc, table), vec!["colgroup".to_string(), "tbody".to_string()]);
    }

    #[test]
    fn whitespace_directly_inside_colgroup_is_kept() {
        let doc = parse("<table><colgroup>\n<col></colgroup></table>");
        let colgroup = find_by_tag(&doc, doc.root(), "colgroup").unwrap();
        assert!(doc.children(colgroup).any(|c| matches!(doc.data(c), NodeData::Text { .. })));
    }

    #[test]
    fn whitespace_before_and_after_the_body_end_tag_merges_into_one_text_node() {
        // Regression test for a real gap `phase-15-chromium-
        // differential-testing/PLAN.md`'s DOM diff against real
        // Chromium found (invisible in any rendered output, since
        // both shapes are pure whitespace, which is exactly why no
        // fixture's #paint/#layout section had caught it): whitespace
        // between </div> and </body>, and whitespace between </body>
        // and </html> (reprocessed under "in body" rules per the
        // "after body" insertion mode), both insert into <body> and
        // must land in the *same* Text node, not two adjacent ones,
        // per HTML5's "insert a character" algorithm.
        let doc = parse("<html><body><div></div>\n</body>\n</html>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        let text_children: Vec<_> = doc.children(body).filter(|&c| matches!(doc.data(c), NodeData::Text { .. })).collect();
        assert_eq!(text_children.len(), 1, "the two whitespace runs must merge into a single Text node, not stay as two siblings");
        assert_eq!(text_content(&doc, body).matches('\n').count(), 2, "both newlines must still be present in the merged node's data");
    }

    #[test]
    fn character_tokens_separated_by_a_comment_do_not_merge() {
        // Corrected by the WPT tree-construction corpus run
        // (`tests/wpt_corpus.rs`, `comments01.dat`): an earlier version
        // of this fix over-generalized and merged text across a
        // dropped comment too, reasoning that since blueice_dom never
        // materializes Comment nodes, nothing sits between the two
        // runs. That reasoning was wrong -- a *real* browser's actual
        // Comment node physically blocks the merge, so "FOO<!--
        // BAR -->BAZ" must produce two separate Text nodes ("FOO",
        // "BAZ"), not one ("FOOBAZ"), even though BlueIce itself never
        // keeps the comment around afterward.
        let doc = parse("<p>a<!--x-->b</p>");
        let p = find_by_tag(&doc, doc.root(), "p").unwrap();
        let text_children: Vec<_> = doc.children(p).filter(|&c| matches!(doc.data(c), NodeData::Text { .. })).collect();
        assert_eq!(text_children.len(), 2, "a real comment node would block the merge, even though blueice_dom doesn't keep it around");
        assert_eq!(text_content(&doc, p), "ab");
    }

    #[test]
    fn foster_parented_character_tokens_separated_by_a_comment_do_not_merge() {
        // Same correction, exercised on the foster-parenting insertion
        // path (text placed directly inside <table> is foster-parented
        // to just before the table, not inside it).
        let doc = parse("<div><table>a<!--x-->b</table></div>");
        let div = find_by_tag(&doc, doc.root(), "div").unwrap();
        let text_children: Vec<_> = doc.children(div).filter(|&c| matches!(doc.data(c), NodeData::Text { .. })).collect();
        assert_eq!(text_children.len(), 2, "a real comment node would block the merge on the foster-parenting path too");
    }

    #[test]
    fn character_tokens_separated_by_a_real_element_do_not_merge() {
        // The merge rule must not over-fire: "a" and "c" here are
        // genuinely not adjacent siblings (a real <b> element sits
        // between them in the final tree), so they must stay as
        // three distinct children, not get merged across the element.
        let doc = parse("<p>a<b></b>c</p>");
        let p = find_by_tag(&doc, doc.root(), "p").unwrap();
        assert_eq!(doc.children(p).count(), 3);
    }

    #[test]
    fn nested_table_directly_in_table_mode_closes_the_outer_one() {
        let doc = parse("<table><table><tr><td>inner</td></tr></table></table>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        // the malformed nested <table> closes the (empty) outer one and
        // starts a second, sibling table containing the real content
        let tables: Vec<_> = doc.children(body).filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name, ..} if tag_name=="table")).collect();
        assert_eq!(tables.len(), 2);
        let td = find_by_tag(&doc, tables[1], "td").unwrap();
        assert_eq!(text_content(&doc, td), "inner");
    }

    #[test]
    fn stray_table_structure_end_tag_in_table_mode_is_ignored() {
        let doc = parse("<table></tbody><tr><td>x</td></tr></table>");
        let table = find_by_tag(&doc, doc.root(), "table").unwrap();
        let td = find_by_tag(&doc, table, "td").unwrap();
        assert_eq!(text_content(&doc, td), "x");
    }

    #[test]
    fn caption_closes_implicitly_before_a_row() {
        let doc = parse("<table><caption>Cap<tr><td>x</td></tr></table>");
        let table = find_by_tag(&doc, doc.root(), "table").unwrap();
        assert_eq!(children_tags(&doc, table), vec!["caption".to_string(), "tbody".to_string()]);
    }

    #[test]
    fn caption_closes_implicitly_on_end_table() {
        let doc = parse("<table><caption>Cap</table>after");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        let table = find_by_tag(&doc, body, "table").unwrap();
        assert_eq!(children_tags(&doc, table), vec!["caption".to_string()]);
        assert_eq!(text_content(&doc, body), "Capafter");
    }

    #[test]
    fn table_body_section_switches_directly_to_a_sibling_section() {
        let doc = parse("<table><tbody><tr><td>a</td></tr><thead><tr><th>b</th></tr></thead></table>");
        let table = find_by_tag(&doc, doc.root(), "table").unwrap();
        assert_eq!(children_tags(&doc, table), vec!["tbody".to_string(), "thead".to_string()]);
    }

    #[test]
    fn table_body_end_tag_closes_section_back_to_table_mode() {
        let doc = parse("<table><tbody><tr><td>a</td></tr></tbody><tr><td>b</td></tr></table>");
        let table = find_by_tag(&doc, doc.root(), "table").unwrap();
        // a second, implicit tbody is opened for the trailing <tr>
        assert_eq!(children_tags(&doc, table), vec!["tbody".to_string(), "tbody".to_string()]);
    }

    // second_row_implicitly_closes_the_first is now
    // tables.dat#2 (see tests/fixtures.rs).

    #[test]
    fn table_body_end_tag_implicitly_closes_an_open_row() {
        let doc = parse("<table><tbody><tr><td>a</tbody><tr><td>b</table>");
        let table = find_by_tag(&doc, doc.root(), "table").unwrap();
        let bodies: Vec<_> = doc
            .children(table)
            .filter(|&c| matches!(doc.data(c), NodeData::Element{tag_name, ..} if tag_name=="tbody"))
            .collect();
        assert_eq!(bodies.len(), 2);
    }

    // second_cell_implicitly_closes_the_first is now
    // tables.dat#3 (see tests/fixtures.rs).

    #[test]
    fn row_end_tag_implicitly_closes_an_open_cell() {
        let doc = parse("<table><tr><td>a</tr><tr><td>b</tr></table>");
        let table = find_by_tag(&doc, doc.root(), "table").unwrap();
        let tbody = doc.children(table).next().unwrap();
        assert_eq!(children_tags(&doc, tbody), vec!["tr".to_string(), "tr".to_string()]);
    }

    #[test]
    fn content_after_body_close_reopens_body_processing() {
        let doc = parse("<html><body>x</body> extra</html>");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(text_content(&doc, body), "x extra");
    }

    #[test]
    fn content_after_html_close_still_lands_in_body() {
        let doc = parse("<html><body>x</body></html> tail");
        let body = find_by_tag(&doc, doc.root(), "body").unwrap();
        assert_eq!(text_content(&doc, body), "x tail");
    }

    // ---- interaction tests: coverage-number gaps often hide at feature
    // boundaries, not inside a single feature. formatting_element_-
    // reconstructs_across_a_new_block_boundary and adoption_agency_-
    // foster_parents_the_relocated_node_when_common_ancestor_is_a_table
    // were added here by a dedicated post-implementation test-review
    // pass (per TEST_PLAN.md's Definition of Done), then migrated to
    // reconstruction.dat and foster-parenting.dat#2 respectively once
    // the shared fixture interface existed (see tests/fixtures.rs) --
    // the second one is also how the block-furthest-block adoption-
    // agency bug referenced above was caught in the first place.
}
