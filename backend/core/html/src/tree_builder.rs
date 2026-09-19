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
//! /object/marquee; frameset-related modes; foreign content (svg/math);
//! comments are never turned into DOM nodes (doesn't affect rendering,
//! which is all the MVP pipeline needs from the DOM). Quirks-mode
//! detection (`quirks_mode`) *is* implemented, but only its narrow
//! tree-construction effect -- whether any `<!DOCTYPE>` token preceded
//! real content, gating `<table>`'s p-closing behavior in "in body" --
//! not the full legacy doctype-name/public-ID/system-ID classification
//! table real engines also consult (see `quirks_mode`'s own docs).
//!
//! **Active-formatting-elements markers and the Noah's Ark clause are
//! implemented** ([`AfeEntry::Marker`], [`TreeBuilder::push_formatting`]):
//! an earlier version of these docs assumed `<template>`/applet/object/
//! marquee were the *only* elements needing a marker, and since none of
//! them are supported, reasoned the active-formatting-elements list
//! never needed one at all. That reasoning was wrong -- `<caption>` and
//! `<td>`/`<th>` also insert a marker per spec, specifically so
//! reconstruction inside a table cell/caption can't reach back out to
//! formatting elements opened before it (`<table><a>x<td>y` must not
//! reconstruct a clone of `<a>` inside the cell), and both are very much
//! in MVP scope. Found the same way as the WPT-corpus bugs below: a real
//! (if uncommon) page shape the algorithm silently got wrong.

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
/// WHATWG's "special" category (https://html.spec.whatwg.org/#formatting),
/// restricted to the MVP element set -- used by both the adoption
/// agency algorithm's furthest-block search and the generic "any other
/// end tag" algorithm's early-abort check. Notably does *not* include
/// `option`/`optgroup` (nor the formatting elements themselves, tracked
/// separately in `FORMATTING_ELEMENTS`) -- an earlier version of
/// `is_special` approximated this as "not a formatting element" instead
/// of this real, enumerated list, which happened to work for the
/// adoption agency's own furthest-block search (misnested formatting
/// rarely interacts with `<option>`/`<optgroup>` at all) but broke once
/// the generic end-tag algorithm started being relied on for `</option>`/
/// `</optgroup>`/`</select>` too (Phase 13's select-content-model
/// rewrite, see `phase-2-mvp-scope/PLAN.md`'s cross-reference): `</optgroup>`
/// scanning past a still-open `<option>` to find the `<optgroup>` below
/// it was wrongly treated as blocked, since the approximation counted
/// `option` as special. Found by the WPT corpus's `tests2.dat#37`.
const SPECIAL_ELEMENTS: &[&str] = &[
    "article",
    "aside",
    "blockquote",
    "body",
    "br",
    "button",
    "caption",
    "col",
    "colgroup",
    "div",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "img",
    "input",
    "li",
    "link",
    "main",
    "meta",
    "nav",
    "ol",
    "p",
    "pre",
    "script",
    "section",
    "select",
    "style",
    "table",
    "tbody",
    "td",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "ul",
];

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

/// An entry in the active-formatting-elements list. Almost always a
/// formatting element; a `Marker` is inserted by `<caption>`/`<td>`/
/// `<th>` specifically to stop reconstruction and the adoption agency
/// algorithm from reaching *past* the table cell/caption boundary back
/// out to formatting elements opened before it -- e.g. `<table><a>x
/// <td>y` must not reconstruct a clone of `<a>` inside the cell. Markers
/// are BlueIce's one exception to the module docs' "no `<template>`/
/// applet/object/marquee means no markers are ever needed" reasoning:
/// that reasoning covers the elements that need a marker in the *full*
/// spec, but table cells/captions need one too and are very much in
/// MVP scope.
#[derive(Clone)]
enum AfeEntry {
    Marker,
    Formatting(FormattingEntry),
}

struct TreeBuilder {
    tokenizer: Tokenizer,
    document: Document,
    open_elements: Vec<NodeId>,
    active_formatting: Vec<AfeEntry>,
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
    /// Set right after inserting a `<pre>` or `<textarea>` element; the
    /// next character token (if any) has a single leading U+000A LINE
    /// FEED stripped from it before insertion -- "Newlines at the start
    /// of pre blocks are ignored as an authoring convenience" (and the
    /// same rule applies to `<textarea>`). Cleared after the next
    /// character token is processed, whether or not it started with one.
    strip_leading_newline: bool,
    /// Whether this document is in quirks mode -- per the reference
    /// `TreeBuilder.java`'s own comment on its one tree-construction
    /// consumer of this flag ("The only quirk. Blame Hixie and Acid2."),
    /// quirks mode affects exactly one tree-construction decision (see
    /// `step_in_body`'s `"table"` arm), unlike its much larger effect on
    /// CSS. Scoped to just the overwhelmingly common real-world trigger
    /// -- no `<!DOCTYPE>` token at all before other content starts (set
    /// in `step_initial`) -- not the full ~60-entry legacy
    /// name/public-ID/system-ID quirks/limited-quirks classification
    /// table `isQuirky`/`isAlmostStandards` implement (a `<!DOCTYPE
    /// html>` or any other doctype token at all is treated as
    /// no-quirks): an "honest cut" matching this project's established
    /// precedent elsewhere (the minimal named-character-reference set,
    /// the MVP HTML element list) of building only what an observed
    /// failing case actually needs, not a full legacy table nothing in
    /// the WPT corpus currently exercises.
    quirks_mode: bool,
}

/// Parses `input` as HTML into a fresh [`Document`], per the tree
/// builder's supported subset (module docs).
pub fn parse(input: &str) -> Document {
    parse_continuing_from(input, 0)
}

/// Like [`parse`], but the resulting document's `NodeId`s start at
/// `next_id` instead of 0 -- for a caller (e.g. `Page::load_html`)
/// replacing an existing document with a freshly-parsed one, so the new
/// document's IDs never collide with the one it's replacing. See
/// [`blueice_dom::Document::new_continuing_from`] for why this matters.
pub fn parse_continuing_from(input: &str, next_id: u64) -> Document {
    let mut tb = TreeBuilder::new(input, next_id);
    tb.run();
    tb.document
}

impl TreeBuilder {
    fn new(input: &str, next_id: u64) -> Self {
        TreeBuilder {
            tokenizer: Tokenizer::new(input),
            document: Document::new_continuing_from(next_id),
            open_elements: Vec::new(),
            active_formatting: Vec::new(),
            mode: Mode::Initial,
            original_mode: Mode::InBody,
            head_element: None,
            form_element: None,
            foster_parenting: false,
            just_saw_dropped_comment_or_doctype: false,
            strip_leading_newline: false,
            quirks_mode: false,
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
        // The pre/textarea leading-newline exception only ever applies to
        // the token immediately following the start tag; anything other
        // than a character token arriving first (a comment, another tag,
        // ...) means the exception no longer applies once we do get to a
        // character token later.
        if !matches!(token, Token::Character(_)) {
            self.strip_leading_newline = false;
        }
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
            Some(t) => SPECIAL_ELEMENTS.contains(&t.as_str()),
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
        if let Some(&table_id) = self
            .open_elements
            .iter()
            .rev()
            .find(|&&id| self.tag_of(id).as_deref() == Some("table"))
        {
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
        let id = self.document.create_node(NodeData::Text {
            data: data.to_string(),
        });
        self.document.insert_before(parent, id, reference);
    }

    /// The position right after the last `Marker` entry in the active-
    /// formatting-elements list, or `0` if there is none -- reconstruction,
    /// the adoption agency's formatting-element search, and the Noah's
    /// Ark clause below all only ever look at the slice from this point
    /// to the end of the list.
    fn afe_scan_start(&self) -> usize {
        self.active_formatting
            .iter()
            .rposition(|e| matches!(e, AfeEntry::Marker))
            .map(|p| p + 1)
            .unwrap_or(0)
    }

    /// Inserts a `Marker` at the end of the active-formatting-elements
    /// list -- WHATWG's own term for this exact step, done by `<caption>`
    /// and `<td>`/`<th>` specifically so that later reconstruction (and
    /// the adoption agency algorithm) can't reach back out past the
    /// table cell/caption boundary to formatting elements opened before
    /// it. See [`AfeEntry::Marker`]'s docs for why table cells need this
    /// despite the module docs' blanket "no markers needed" reasoning.
    fn insert_afe_marker(&mut self) {
        self.active_formatting.push(AfeEntry::Marker);
    }

    /// The "Noah's Ark clause" (WHATWG's own name for this step): if
    /// three elements with the same tag name and attributes as this one
    /// are already in the active-formatting-elements list (since the
    /// last marker, or the start of the list if there is none), remove
    /// the earliest of them before adding the new one. Without this,
    /// `<p><b><b><b><b><p>x` would reconstruct all four `<b>`s under the
    /// second `<p>` instead of the spec-mandated three.
    fn push_formatting(&mut self, id: NodeId, tag: &str, attrs: Vec<(String, String)>) {
        let scan_start = self.afe_scan_start();
        let matching: Vec<usize> = self.active_formatting[scan_start..]
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                AfeEntry::Formatting((_, t, a)) if t == tag && Self::attrs_equal(a, &attrs) => {
                    Some(scan_start + i)
                }
                _ => None,
            })
            .collect();
        if matching.len() >= 3 {
            self.active_formatting.remove(matching[0]);
        }
        self.active_formatting
            .push(AfeEntry::Formatting((id, tag.to_string(), attrs)));
    }

    fn attrs_equal(a: &[(String, String)], b: &[(String, String)]) -> bool {
        a.len() == b.len() && a.iter().all(|pair| b.contains(pair))
    }

    /// Consumes [`Self::strip_leading_newline`] against one incoming
    /// character-token string: if set, strips a single leading `\n` (if
    /// present) and always clears the flag, since the exception only
    /// ever applies to the token immediately following `<pre>`/
    /// `<textarea>`'s start tag.
    fn consume_leading_newline_strip(&mut self, s: String) -> String {
        if !self.strip_leading_newline {
            return s;
        }
        self.strip_leading_newline = false;
        s.strip_prefix('\n').map(str::to_string).unwrap_or(s)
    }

    fn switch_to_text_mode(&mut self, name: &str, attrs: Vec<(String, String)>) {
        self.insert_element(name, attrs);
        let model = if RCDATA_ELEMENTS.contains(&name) {
            ContentModel::Rcdata
        } else if name == "script" {
            ContentModel::ScriptData
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
            if DEFAULT_SCOPE_BLOCKERS.contains(&t.as_str()) || extra_blockers.contains(&t.as_str())
            {
                return false;
            }
        }
        false
    }

    /// WHATWG's narrower "in table scope" (used only for the
    /// table/tbody/tfoot/thead/tr end-tag rules) -- unlike the general
    /// "in scope" algorithm ([`Self::has_tag_in_scope`]), `td`/`th`/
    /// `caption` do *not* stop the walk here, since a `<tbody>` is always
    /// an ancestor of any cell inside it and must still be found by,
    /// e.g., a stray `</tbody>` reached while inside one of its own
    /// cells (`has_tag_in_scope` would wrongly stop at the enclosing
    /// `<td>` and report nothing in scope).
    fn has_tag_in_table_scope(&self, tag: &str) -> bool {
        for &id in self.open_elements.iter().rev() {
            let Some(t) = self.tag_of(id) else { continue };
            if t == tag {
                return true;
            }
            if matches!(t.as_str(), "html" | "table") {
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
        self.clear_afe_up_to_last_marker();
        self.mode = Mode::InRow;
    }

    /// WHATWG's "clear the list of active formatting elements up to the
    /// last marker": pop entries off the end of the list, including the
    /// marker itself, until a marker has been popped (or the list is
    /// empty). Run when a table cell or caption closes, so a *later*,
    /// unrelated cell/caption doesn't reconstruct formatting elements
    /// left dangling from a previous one.
    fn clear_afe_up_to_last_marker(&mut self) {
        while let Some(entry) = self.active_formatting.pop() {
            if matches!(entry, AfeEntry::Marker) {
                break;
            }
        }
    }

    fn close_table_section(&mut self) {
        while let Some(&top) = self.open_elements.last() {
            let is_section = matches!(
                self.tag_of(top).as_deref(),
                Some("tbody") | Some("thead") | Some("tfoot")
            );
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
    /// elements -- WHATWG's "reset the insertion mode appropriately",
    /// restricted to the table/select frames BlueIce's MVP scope can
    /// actually have on the stack (no `template`/`frameset`, no
    /// fragment-parsing "context element"). Two different call shapes
    /// reach this: `pop_until_and_including("table")` callers, which by
    /// construction have already popped every table-internal frame along
    /// with `table` itself (so this walk only ever finds `body`/`html`
    /// for them); and `pop_until_and_including("select")` callers
    /// (`step_in_select`/`step_in_select_in_table`), which can leave a
    /// `table`/`tbody`/`tr`/... frame exposed as the new current node --
    /// e.g. `<table><tbody><select><tr>` closing the `<select>` must
    /// land back in `InTableBody`, not `InBody`.
    fn reset_insertion_mode(&mut self) {
        for &id in self.open_elements.iter().rev() {
            match self.tag_of(id).as_deref() {
                Some("td") | Some("th") => {
                    self.mode = Mode::InCell;
                    return;
                }
                Some("tr") => {
                    self.mode = Mode::InRow;
                    return;
                }
                Some("tbody") | Some("thead") | Some("tfoot") => {
                    self.mode = Mode::InTableBody;
                    return;
                }
                Some("caption") => {
                    self.mode = Mode::InCaption;
                    return;
                }
                Some("colgroup") => {
                    self.mode = Mode::InColumnGroup;
                    return;
                }
                Some("table") => {
                    self.mode = Mode::InTable;
                    return;
                }
                Some("body") => {
                    self.mode = Mode::InBody;
                    return;
                }
                Some("html") => {
                    self.mode = if self.head_element.is_some() {
                        Mode::AfterHead
                    } else {
                        Mode::BeforeHead
                    };
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
        // A `Marker` counts as "already satisfied" here, exactly like an
        // already-open formatting element does -- it's the boundary
        // `<caption>`/`<td>`/`<th>` insert specifically to stop
        // reconstruction from reaching back out to formatting elements
        // opened before the cell/caption.
        let is_open_or_marker = |tb: &Self, i: usize| match &tb.active_formatting[i] {
            AfeEntry::Marker => true,
            AfeEntry::Formatting((id, _, _)) => tb.open_elements.contains(id),
        };
        let last = self.active_formatting.len() - 1;
        if is_open_or_marker(self, last) {
            return;
        }
        let mut first = last;
        while first > 0 && !is_open_or_marker(self, first - 1) {
            first -= 1;
        }
        for i in first..=last {
            let AfeEntry::Formatting((_, tag, attrs)) = self.active_formatting[i].clone() else {
                continue;
            };
            let new_id = self.insert_element(&tag, attrs.clone());
            self.active_formatting[i] = AfeEntry::Formatting((new_id, tag, attrs));
        }
    }

    /// Looks up a `Formatting` entry's `(id, tag, attrs)` by stack
    /// position, panicking if that position holds a `Marker` --
    /// callers only ever index positions they've already confirmed are
    /// `Formatting` entries (e.g. results of [`Self::afe_formatting_rposition`]).
    fn afe_formatting_at(&self, pos: usize) -> &FormattingEntry {
        match &self.active_formatting[pos] {
            AfeEntry::Formatting(e) => e,
            AfeEntry::Marker => unreachable!("expected a formatting entry, found a marker"),
        }
    }

    /// The last position at or after [`Self::afe_scan_start`] (i.e. not
    /// stepping past a marker) holding a `Formatting` entry matching
    /// `tag` -- the adoption agency algorithm's "last element in the
    /// list of active formatting elements ... that has the tag name
    /// subject" search, spec-bounded to never reach past the most
    /// recent `<caption>`/`<td>`/`<th>` marker.
    fn afe_formatting_rposition(&self, tag: &str) -> Option<usize> {
        let scan_start = self.afe_scan_start();
        self.active_formatting[scan_start..]
            .iter()
            .rposition(|e| matches!(e, AfeEntry::Formatting((_, t, _)) if t == tag))
            .map(|p| scan_start + p)
    }

    /// The adoption agency algorithm (WHATWG HTML5 §13.2.5.2), for an end
    /// tag naming a formatting element. See module docs for what's
    /// intentionally simplified relative to the full spec (no Noah's
    /// Ark clause beyond [`Self::push_formatting`]'s handling).
    fn adoption_agency(&mut self, tag: &str) {
        // Step 2 (a fast path spec calls out explicitly): if the current
        // node already matches `tag` but was never tracked as an active
        // formatting element (e.g. a plain, unformatted element that
        // just happens to share the tag name), just pop it and return --
        // skip the whole algorithm below.
        if let Some(&current) = self.open_elements.last() {
            let tracked = self
                .active_formatting
                .iter()
                .any(|e| matches!(e, AfeEntry::Formatting((id, _, _)) if *id == current));
            if self.tag_of(current).as_deref() == Some(tag) && !tracked {
                self.open_elements.pop();
                return;
            }
        }

        for _ in 0..8 {
            let Some(fe_pos) = self.afe_formatting_rposition(tag) else {
                self.any_other_end_tag(tag);
                return;
            };
            let fe_id = self.afe_formatting_at(fe_pos).0;

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

            let between: Vec<NodeId> =
                self.open_elements[fe_stack_pos + 1..furthest_block_pos].to_vec();
            // `Bookmark` mirrors Blink's `HTMLFormattingElementList::Bookmark`
            // (`html_formatting_element_list.h`): a position tracked
            // *relative to a specific entry*, resolved to a concrete
            // index only once, right before the final insert -- not a
            // raw `usize` snapshotted up front. Indices into
            // `active_formatting` shift every time an earlier entry is
            // removed or inserted during the inner loop below, so a
            // stale raw index silently drifts; re-resolving by node
            // identity at the point of use can't drift.
            enum Bookmark {
                AtFormattingElement,
                AfterNode(NodeId),
            }
            let mut bookmark = Bookmark::AtFormattingElement;
            let mut last_node = furthest_block_id;
            let mut iterations = 0;

            for &node_id in between.iter().rev() {
                iterations += 1;
                let af_pos = self
                    .active_formatting
                    .iter()
                    .position(|e| matches!(e, AfeEntry::Formatting((id, _, _)) if *id == node_id));
                let stack_pos_of =
                    |tb: &Self, id: NodeId| tb.open_elements.iter().position(|&x| x == id);

                let Some(af_pos) = af_pos else {
                    // Not (or no longer) an active formatting element:
                    // remove it from the stack only, and move on --
                    // never cloned, never reparented.
                    if let Some(p) = stack_pos_of(self, node_id) {
                        self.open_elements.remove(p);
                    }
                    continue;
                };
                if iterations > 3 {
                    // Spec's inner-loop iteration cap: once we're deep
                    // enough in a long misnested chain, age this entry
                    // out of the active-formatting list *and* the stack
                    // rather than cloning it -- same as the "not in AFE"
                    // branch above, just reached by aging out instead.
                    self.active_formatting.remove(af_pos);
                    if let Some(p) = stack_pos_of(self, node_id) {
                        self.open_elements.remove(p);
                    }
                    continue;
                }

                // Clone `node_id`'s element and replace *both* its
                // active-formatting entry and its stack-of-open-elements
                // entry with the clone, in place -- the stack entry must
                // be replaced, not just dropped, since later iterations
                // (and, for the outermost node, the final reparent step)
                // still need a valid current stack position for it.
                let (_, node_tag, node_attrs) = self.afe_formatting_at(af_pos).clone();
                let new_node = self.document.create_node(NodeData::Element {
                    tag_name: node_tag.clone(),
                    attributes: node_attrs.clone(),
                });
                self.active_formatting[af_pos] =
                    AfeEntry::Formatting((new_node, node_tag, node_attrs));
                if let Some(p) = stack_pos_of(self, node_id) {
                    self.open_elements[p] = new_node;
                }

                if last_node == furthest_block_id {
                    bookmark = Bookmark::AfterNode(new_node);
                }

                if self.document.parent(last_node).is_some() {
                    self.document.detach(last_node);
                }
                self.document.append_child(new_node, last_node);
                last_node = new_node;
            }

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

            let (_, fe_tag, fe_attrs) = self.afe_formatting_at(fe_pos).clone();
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

            // Resolve the bookmark by identity, right before mutating
            // the list, so it reflects every removal the inner loop just
            // did -- then account for `fe`'s own removal (a single
            // earlier entry disappearing shifts everything after it down
            // by one) explicitly, rather than clamping to length and
            // hoping that coincidentally lands right.
            let raw_insert_at = match bookmark {
                Bookmark::AtFormattingElement => fe_pos,
                Bookmark::AfterNode(id) => self
                    .active_formatting
                    .iter()
                    .position(|e| matches!(e, AfeEntry::Formatting((x, _, _)) if *x == id))
                    .map(|p| p + 1)
                    .unwrap_or(fe_pos),
            };
            let fe_pos_now = self
                .active_formatting
                .iter()
                .position(|e| matches!(e, AfeEntry::Formatting((id, _, _)) if *id == fe_id))
                .unwrap();
            self.active_formatting.remove(fe_pos_now);
            let insert_at = if raw_insert_at > fe_pos_now {
                raw_insert_at - 1
            } else {
                raw_insert_at
            };
            let insert_at = insert_at.min(self.active_formatting.len());
            self.active_formatting
                .insert(insert_at, AfeEntry::Formatting((new_fe, fe_tag, fe_attrs)));

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
            let fb_pos_now = self
                .open_elements
                .iter()
                .position(|&id| id == furthest_block_id)
                .unwrap();
            self.open_elements.insert(fb_pos_now + 1, new_fe);
        }
    }

    // ---- insertion modes ----

    /// Shared by [`Self::step_before_html`] and [`Self::step_before_head`]:
    /// unlike the later head/body-area modes (`step_in_head`/
    /// `step_after_head`/...), whitespace here is never inserted as text
    /// -- there's no element for it to belong to yet -- it's simply
    /// dropped. Still needs the same mixed-run splitting those modes use
    /// (see [`Self::split_leading_whitespace`]'s own docs): a batched
    /// character token like `"\n]>"` must drop only the leading `"\n"`
    /// and let `"]>"` alone trigger the mode's "anything else" fallback,
    /// not treat the whole run as non-whitespace content. Found by the
    /// WPT corpus's `doctype01.dat#30` (a bogus DOCTYPE followed by a
    /// lone newline then stray text): without this, that leading
    /// newline rode along with the non-whitespace content through every
    /// later mode transition and wrongly ended up materialized as a
    /// text node inside the implicit `<head>`, instead of being dropped
    /// per spec before `<html>`/`<head>` even exist.
    fn split_off_dropped_whitespace(token: Token) -> Result<StepResult, Token> {
        if let Token::Character(s) = &token {
            if s.trim().is_empty() {
                return Ok(StepResult::Done);
            }
            if let Some((_ws, rest)) = Self::split_leading_whitespace(s) {
                return Ok(StepResult::Reprocess(Token::Character(rest.to_string())));
            }
        }
        Err(token)
    }

    fn step_initial(&mut self, token: Token) -> StepResult {
        match &token {
            // Spec: a DOCTYPE token *also* switches the insertion mode
            // to "before html" (not just a `Done`-and-stay-put no-op) --
            // this distinction only matters for `quirks_mode` below: if
            // this didn't transition the mode itself, the next
            // real-content token would fall through the catch-all arm
            // regardless of whether a doctype had just been seen,
            // wrongly setting quirks mode even for a perfectly ordinary
            // `<!doctype html>` document.
            Token::Doctype => {
                self.mode = Mode::BeforeHtml;
                StepResult::Done
            }
            Token::Comment => StepResult::Done,
            Token::Character(s) if s.trim().is_empty() => StepResult::Done,
            _ => {
                // Spec: reaching any other token in "initial" mode means
                // no `<!DOCTYPE>` token ever appeared -- the
                // overwhelmingly common real-world quirks-mode trigger
                // (see `quirks_mode`'s own docs for the narrower legacy
                // doctype-name/public-ID table this doesn't implement).
                self.quirks_mode = true;
                self.mode = Mode::BeforeHtml;
                StepResult::Reprocess(token)
            }
        }
    }

    fn step_before_html(&mut self, token: Token) -> StepResult {
        let token = match Self::split_off_dropped_whitespace(token) {
            Ok(result) => return result,
            Err(token) => token,
        };
        match &token {
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::StartTag { name, attrs, .. } if name == "html" => {
                self.insert_element("html", attrs.clone());
                self.mode = Mode::BeforeHead;
                StepResult::Done
            }
            Token::EndTag { name } if !matches!(name.as_str(), "head" | "body" | "html" | "br") => {
                StepResult::Done
            }
            _ => {
                self.insert_element("html", vec![]);
                self.mode = Mode::BeforeHead;
                StepResult::Reprocess(token)
            }
        }
    }

    fn step_before_head(&mut self, token: Token) -> StepResult {
        let token = match Self::split_off_dropped_whitespace(token) {
            Ok(result) => return result,
            Err(token) => token,
        };
        match &token {
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::StartTag { name, .. } if name == "html" => self.step_in_body(token),
            Token::StartTag { name, attrs, .. } if name == "head" => {
                let id = self.insert_element("head", attrs.clone());
                self.head_element = Some(id);
                self.mode = Mode::InHead;
                StepResult::Done
            }
            Token::EndTag { name } if !matches!(name.as_str(), "head" | "body" | "html" | "br") => {
                StepResult::Done
            }
            _ => {
                let id = self.insert_element("head", vec![]);
                self.head_element = Some(id);
                self.mode = Mode::InHead;
                StepResult::Reprocess(token)
            }
        }
    }

    /// Splits a mixed character-token string at the boundary between its
    /// leading run of whitespace and the first non-whitespace character.
    /// A real (per-character) tokenizer lets earlier whitespace insert
    /// under whatever whitespace-specific rule the current insertion
    /// mode has (e.g. "in head"/"after head"/"after body" all keep
    /// whitespace where it is) while only the first non-whitespace
    /// character -- and everything from there on -- triggers that
    /// mode's "anything else" fallback (popping `<head>`, opening an
    /// implicit `<body>`, ...). blueice's tokenizer instead batches a
    /// whole run of characters into one token, so those two behaviors
    /// need to be reconstructed by splitting the batched string at the
    /// same boundary before reprocessing. Returns `None` when `s` has no
    /// leading whitespace to split off (the whitespace-only case is
    /// already handled by each mode's own dedicated arm).
    fn split_leading_whitespace(s: &str) -> Option<(&str, &str)> {
        let rest = s.trim_start();
        if rest.len() == s.len() {
            None
        } else {
            Some((&s[..s.len() - rest.len()], rest))
        }
    }

    /// Per spec, "in body" and "in select" (unlike the ordinary "data
    /// state" tokenizer rule, which emits a literal NUL character token
    /// -- see `tokenizer.rs`) drop any U+0000 NULL character token
    /// outright as a parse error, rather than inserting it (as a literal
    /// control character) or replacing it with U+FFFD.
    fn strip_null_characters(s: &str) -> String {
        if s.contains('\0') {
            s.replace('\0', "")
        } else {
            s.to_string()
        }
    }

    fn step_in_head(&mut self, token: Token) -> StepResult {
        if let Token::Character(s) = &token {
            if s.trim().is_empty() {
                self.insert_text(s);
                return StepResult::Done;
            }
            if let Some((ws, rest)) = Self::split_leading_whitespace(s) {
                self.insert_text(ws);
                return StepResult::Reprocess(Token::Character(rest.to_string()));
            }
        }
        match &token {
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::StartTag { name, .. } if name == "html" => self.step_in_body(token),
            Token::StartTag { name, attrs, .. } if matches!(name.as_str(), "meta" | "link") => {
                self.insert_element(name, attrs.clone());
                StepResult::Done
            }
            Token::StartTag { name, attrs, .. }
                if matches!(name.as_str(), "title" | "style" | "script") =>
            {
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
            Token::EndTag { name } if !matches!(name.as_str(), "body" | "html" | "br") => {
                StepResult::Done
            }
            _ => {
                self.open_elements.pop();
                self.mode = Mode::AfterHead;
                StepResult::Reprocess(token)
            }
        }
    }

    fn step_after_head(&mut self, token: Token) -> StepResult {
        if let Token::Character(s) = &token {
            if s.trim().is_empty() {
                self.insert_text(s);
                return StepResult::Done;
            }
            if let Some((ws, rest)) = Self::split_leading_whitespace(s) {
                self.insert_text(ws);
                return StepResult::Reprocess(Token::Character(rest.to_string()));
            }
        }
        match &token {
            Token::Doctype | Token::Comment => StepResult::Done,
            Token::StartTag { name, .. } if name == "html" => self.step_in_body(token),
            Token::StartTag { name, attrs, .. } if name == "body" => {
                self.insert_element("body", attrs.clone());
                self.mode = Mode::InBody;
                StepResult::Done
            }
            Token::StartTag { name, .. } if name == "head" => StepResult::Done,
            Token::StartTag { name, .. }
                if matches!(
                    name.as_str(),
                    "meta" | "link" | "title" | "style" | "script"
                ) =>
            {
                // Spec: these still belong in `<head>` even after `</head>`
                // has already closed it -- temporarily re-push the head
                // element, delegate to "in head" rules, then remove it from
                // the stack again (it may not be the current node anymore,
                // e.g. once `title`/`style`/`script` pushed a Text-mode
                // element above it). Without this, `<head></head><title>X`
                // wrongly opened an implicit `<body>` and put `title` there.
                let Some(head_id) = self.head_element else {
                    self.insert_element("body", vec![]);
                    self.mode = Mode::InBody;
                    return StepResult::Reprocess(token.clone());
                };
                self.open_elements.push(head_id);
                let result = self.step_in_head(token.clone());
                self.open_elements.retain(|&id| id != head_id);
                result
            }
            Token::EndTag { name } if !matches!(name.as_str(), "body" | "html" | "br") => {
                StepResult::Done
            }
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
                let s = self.consume_leading_newline_strip(s.clone());
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
                let s = Self::strip_null_characters(&s);
                self.reconstruct_active_formatting_elements();
                let s = self.consume_leading_newline_strip(s);
                self.insert_text(&s);
                StepResult::Done
            }
            Token::StartTag { name, attrs, .. } => self.start_tag_in_body(&name, attrs),
            Token::EndTag { name } => self.end_tag_in_body(&name),
        }
    }

    fn start_tag_in_body(&mut self, name: &str, attrs: Vec<(String, String)>) -> StepResult {
        match name {
            "html" => {
                // Spec: a second, stray `<html>` start tag doesn't open
                // a new element -- it merges any attribute not already
                // present onto the *existing* (first) html element,
                // leaving already-set attributes untouched.
                if let Some(&html_id) = self.open_elements.first() {
                    if let NodeData::Element { attributes, .. } = self.document.data_mut(html_id) {
                        for (k, v) in attrs {
                            if !attributes.iter().any(|(ek, _)| *ek == k) {
                                attributes.push((k, v));
                            }
                        }
                    }
                }
                StepResult::Done
            }
            "body" => {
                // Spec: a second, stray `<body>` start tag doesn't open
                // a new element either -- same merge-onto-the-existing-
                // element treatment as a second `<html>` tag above,
                // targeting the second element on the stack (the one
                // right above `<html>`, which is body in every case MVP
                // scope can reach -- no `<frameset>` support).
                if let Some(&body_id) = self.open_elements.get(1) {
                    if let NodeData::Element {
                        tag_name,
                        attributes,
                    } = self.document.data_mut(body_id)
                    {
                        if tag_name == "body" {
                            for (k, v) in attrs {
                                if !attributes.iter().any(|(ek, _)| *ek == k) {
                                    attributes.push((k, v));
                                }
                            }
                        }
                    }
                }
                StepResult::Done
            }
            "head" => StepResult::Done,
            // Spec: these table-structure-only tags have no valid
            // meaning directly in "in body" content (only inside an
            // actual table, handled by the table-family insertion
            // modes) -- ignored outright here, not inserted as
            // ordinary elements. E.g. a stray `<col>` after `</table>`
            // has already closed the table must vanish, not become a
            // body-level child.
            "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr" => {
                StepResult::Done
            }
            "table" => {
                // Spec: closing an open `<p>` here is conditional on the
                // document *not* being in quirks mode -- confirmed
                // against both the WPT corpus (`<!doctype html><p><table>`
                // closes p, table becomes p's sibling; the no-doctype
                // case nests table inside p instead) and the reference
                // `TreeBuilder.java`'s own comment on this exact check
                // ("The only quirk. Blame Hixie and Acid2.").
                if !self.quirks_mode && self.has_p_in_button_scope() {
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
            "button" => {
                if self.has_tag_in_scope("button", &[]) {
                    self.generate_implied_end_tags(None);
                    self.pop_until_and_including("button");
                }
                self.reconstruct_active_formatting_elements();
                self.insert_element("button", attrs);
                StepResult::Done
            }
            "a" => {
                if let Some(fe_pos) = self.afe_formatting_rposition("a") {
                    let existing_a = self.afe_formatting_at(fe_pos).0;
                    self.adoption_agency("a");
                    // Spec's `<a>`-specific start-tag rule, distinct from
                    // the generic formatting-element handling other tags
                    // (`<b>`, `<i>`, ...) share: after running the
                    // adoption agency algorithm, unconditionally remove
                    // the *original* `<a>` from both the stack of open
                    // elements and the active-formatting list if the
                    // algorithm didn't already do so itself. This matters
                    // because the algorithm can return as a no-op --
                    // e.g. its "not in scope" branch (`adoption-agency-4.4`
                    // in html5lib's own step numbering), reached when an
                    // intervening `<table>` sits between the open `<a>`
                    // and the top of the stack -- leaving that stale `<a>`
                    // behind for this second `<a>` to still clean up.
                    // Found by tracing the real html5lib reference
                    // implementation's `startTagA` against WPT's
                    // `tests1.dat#90` (`<a><table><a></table><p><a>...`),
                    // whose expected tree only makes sense once this step
                    // runs even though the adoption agency call itself
                    // did nothing that time.
                    self.open_elements.retain(|&id| id != existing_a);
                    self.active_formatting.retain(
                        |e| !matches!(e, AfeEntry::Formatting((id, _, _)) if *id == existing_a),
                    );
                }
                self.reconstruct_active_formatting_elements();
                let id = self.insert_element("a", attrs.clone());
                self.push_formatting(id, "a", attrs);
                StepResult::Done
            }
            // Spec: `<title>` (like `<script>`/`<style>`) is processed
            // via "in head" rules even when it turns up directly in body
            // content -- most importantly, it still gets RCDATA content-
            // model treatment, so a stray `</body>`/`</html>`/etc. inside
            // it stays literal text up to the real `</title>`, instead
            // of being tokenized as a real (and disruptive) end tag.
            "textarea" | "script" | "style" | "title" => {
                self.switch_to_text_mode(name, attrs);
                if name == "textarea" {
                    self.strip_leading_newline = true;
                }
                StepResult::Done
            }
            "pre" => {
                if self.has_p_in_button_scope() {
                    self.close_p_element();
                }
                self.insert_element("pre", attrs);
                self.strip_leading_newline = true;
                StepResult::Done
            }
            // `<select>`/`<option>`/`<optgroup>`/`<hr>`'s "has a select
            // in scope" checks below, and `<select>` no longer switching
            // `self.mode` at all, reflect the *current* spec: the old
            // dedicated "in select"/"in select in table" insertion modes
            // (which this project's earlier implementation, and the
            // html5lib reference implementation this whole file's design
            // was originally cross-checked against, both still modeled)
            // were removed from the living standard as part of the
            // "Customizable Select" feature -- confirmed directly against
            // the current spec source (`whatwg/html`'s `source` file:
            // `reset the insertion mode appropriately` and the insertion-
            // mode heading list both have no "select" case at all
            // anymore). `<select>`'s content model is no longer enforced
            // by the parser refusing to build non-`<option>` nodes -- a
            // `<div>`/`<b>`/`<img>` reached while a `<select>` is open now
            // falls through to plain "in body" processing like any other
            // element, exactly the ordinary "any other start tag" rule
            // with no select-awareness at all. `<option>`/`<optgroup>`/
            // `<hr>` are the only elements that still branch on "is a
            // select in scope", and that branch now lives directly in
            // this match rather than a separate mode's own dispatch.
            "select" => {
                if self.has_tag_in_scope("select", &[]) {
                    self.pop_until_and_including("select");
                } else {
                    self.reconstruct_active_formatting_elements();
                    self.insert_element("select", attrs);
                }
                StepResult::Done
            }
            "option" => {
                if self.has_tag_in_scope("select", &[]) {
                    self.generate_implied_end_tags(Some("optgroup"));
                } else if self.is_current("option") {
                    self.open_elements.pop();
                }
                self.reconstruct_active_formatting_elements();
                self.insert_element("option", attrs);
                StepResult::Done
            }
            "optgroup" => {
                if self.has_tag_in_scope("select", &[]) {
                    self.generate_implied_end_tags(None);
                } else if self.is_current("option") {
                    self.open_elements.pop();
                }
                self.reconstruct_active_formatting_elements();
                self.insert_element("optgroup", attrs);
                StepResult::Done
            }
            "hr" => {
                if self.has_p_in_button_scope() {
                    self.close_p_element();
                }
                if self.has_tag_in_scope("select", &[]) {
                    self.generate_implied_end_tags(None);
                }
                self.insert_element("hr", attrs);
                StepResult::Done
            }
            "input" => {
                if self.has_tag_in_scope("select", &[]) {
                    self.pop_until_and_including("select");
                }
                self.reconstruct_active_formatting_elements();
                self.insert_element("input", attrs);
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
                if HEADING_ELEMENTS
                    .contains(&self.tag_of(self.current_node()).as_deref().unwrap_or(""))
                {
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
                    StepResult::Reprocess(Token::EndTag {
                        name: name.to_string(),
                    })
                } else {
                    StepResult::Done
                }
            }
            "br" => {
                // Spec: a stray `</br>` end tag doesn't behave like a
                // normal end tag at all -- it's converted into inserting
                // a `<br>` element, exactly as if a `<br>` start tag had
                // been seen (dropping any attributes the bogus end tag
                // carried, which the tokenizer already discards for end
                // tags anyway). Real pages that typo `</br>` still get
                // the line break they meant to insert.
                self.reconstruct_active_formatting_elements();
                self.insert_element("br", vec![]);
                StepResult::Done
            }
            "p" => {
                if !self.has_p_in_button_scope() {
                    // Spec: a stray `</p>` with no matching open `<p>` is a
                    // parse error, but still inserts an (empty) `<p>` before
                    // immediately closing it -- not simply ignored.
                    self.insert_element("p", vec![]);
                }
                self.close_p_element();
                StepResult::Done
            }
            "li" => {
                if self.has_li_in_list_item_scope() {
                    self.generate_implied_end_tags(Some("li"));
                    self.pop_until_and_including("li");
                }
                StepResult::Done
            }
            "button" => {
                if self.has_tag_in_scope("button", &[]) {
                    self.generate_implied_end_tags(None);
                    self.pop_until_and_including("button");
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

    /// WHATWG's "clear the stack back to a table [body/row] context":
    /// pop elements off the stack of open elements (never touching the
    /// DOM they already belong to) until the current node matches one of
    /// `stop_tags` or is `html`. Several "in table"/"in table body"/
    /// "in row" rules call this before inserting a table-structure
    /// element, specifically so that non-table content parsed in
    /// between (most commonly a formatting element like `<a>` foster-
    /// parented in front of the table -- `<table><a>x<td>y`) doesn't
    /// stay the "current node" and swallow the new element as its own
    /// child instead of the table structure's.
    fn clear_stack_back_to(&mut self, stop_tags: &[&str]) {
        while let Some(&top) = self.open_elements.last() {
            if self
                .tag_of(top)
                .as_deref()
                .is_some_and(|t| t == "html" || stop_tags.contains(&t))
            {
                break;
            }
            self.open_elements.pop();
        }
    }

    fn step_in_table(&mut self, token: Token) -> StepResult {
        match token {
            Token::Character(s) => {
                let s = Self::strip_null_characters(&s);
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
                self.clear_stack_back_to(&["table"]);
                self.insert_afe_marker();
                self.insert_element("caption", attrs);
                self.mode = Mode::InCaption;
                StepResult::Done
            }
            Token::StartTag { name, attrs, .. } if name == "colgroup" => {
                self.clear_stack_back_to(&["table"]);
                self.insert_element("colgroup", attrs);
                self.mode = Mode::InColumnGroup;
                StepResult::Done
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if name == "col" => {
                self.clear_stack_back_to(&["table"]);
                self.insert_element("colgroup", vec![]);
                self.mode = Mode::InColumnGroup;
                StepResult::Reprocess(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
            }
            Token::StartTag { name, attrs, .. }
                if matches!(name.as_str(), "tbody" | "thead" | "tfoot") =>
            {
                self.clear_stack_back_to(&["table"]);
                self.insert_element(&name, attrs);
                self.mode = Mode::InTableBody;
                StepResult::Done
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if matches!(name.as_str(), "tr" | "td" | "th") => {
                self.clear_stack_back_to(&["table"]);
                self.insert_element("tbody", vec![]);
                self.mode = Mode::InTableBody;
                StepResult::Reprocess(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if name == "table" => {
                if self.has_tag_in_scope("table", &[]) {
                    self.pop_until_and_including("table");
                    self.reset_insertion_mode();
                }
                StepResult::Reprocess(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
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
                    "body"
                        | "caption"
                        | "col"
                        | "colgroup"
                        | "html"
                        | "tbody"
                        | "td"
                        | "tfoot"
                        | "th"
                        | "thead"
                        | "tr"
                ) =>
            {
                StepResult::Done
            }
            // Spec: `<style>`/`<script>` directly inside `<table>` (not a
            // cell/caption) are processed via "in head" rules -- inserted
            // as a child of the table element itself -- not treated like
            // ordinary body content needing foster-parenting out in front
            // of the table.
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if matches!(name.as_str(), "style" | "script") => {
                self.step_in_head(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
            }
            // Spec carve-out: `<input type="hidden">` directly inside
            // `<table>` is inserted normally (as the table's own child)
            // rather than foster-parented like other stray content --
            // real pages rely on this for CSRF-token-style hidden inputs
            // placed right inside a `<table>`, before any row.
            Token::StartTag { name, attrs, .. }
                if name == "input"
                    && attrs
                        .iter()
                        .any(|(k, v)| k == "type" && v.eq_ignore_ascii_case("hidden")) =>
            {
                self.insert_element("input", attrs);
                StepResult::Done
            }
            // Spec carve-out, parallel to the `<input type="hidden">` one
            // above but with its own extra wrinkle: `<form>` directly
            // inside `<table>` is also inserted normally rather than
            // foster-parented, but -- unlike every other "in table"
            // carve-out -- it's popped straight back off the stack of
            // open elements immediately after, so it never becomes an
            // open ancestor of whatever the table's *rows* contain. The
            // `form_element` pointer is still respected (ignore outright
            // if a `<form>` is already open elsewhere in the document,
            // the same "in body" rule `step_in_body`'s own `"form"` arm
            // enforces) since real pages nesting a stray second `<form>`
            // inside a `<table>` shouldn't silently get two live forms.
            Token::StartTag { name, attrs, .. } if name == "form" => {
                if self.form_element.is_some() {
                    return StepResult::Done;
                }
                let id = self.insert_element("form", attrs);
                self.form_element = Some(id);
                self.open_elements.pop();
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
                    self.clear_afe_up_to_last_marker();
                }
                self.mode = Mode::InTable;
                StepResult::Done
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if matches!(
                name.as_str(),
                "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr"
            ) =>
            {
                if self.has_tag_in_scope("caption", &[]) {
                    self.generate_implied_end_tags(None);
                    self.pop_until_and_including("caption");
                    self.clear_afe_up_to_last_marker();
                    self.mode = Mode::InTable;
                }
                StepResult::Reprocess(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
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
        if let Token::Character(s) = &token {
            if s.trim().is_empty() {
                self.insert_text(s);
                return StepResult::Done;
            }
            if let Some((ws, rest)) = Self::split_leading_whitespace(s) {
                self.insert_text(ws);
                return StepResult::Reprocess(Token::Character(rest.to_string()));
            }
        }
        match token {
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
                self.clear_stack_back_to(&["tbody", "thead", "tfoot"]);
                self.insert_element("tr", attrs);
                self.mode = Mode::InRow;
                StepResult::Done
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if matches!(name.as_str(), "td" | "th") => {
                self.clear_stack_back_to(&["tbody", "thead", "tfoot"]);
                self.insert_element("tr", vec![]);
                self.mode = Mode::InRow;
                StepResult::Reprocess(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if matches!(name.as_str(), "tbody" | "thead" | "tfoot") => {
                self.close_table_section();
                self.mode = Mode::InTable;
                StepResult::Reprocess(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
            }
            Token::EndTag { name } if matches!(name.as_str(), "tbody" | "thead" | "tfoot") => {
                if self.has_tag_in_scope(&name, &[]) {
                    self.close_table_section();
                }
                self.mode = Mode::InTable;
                StepResult::Done
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if matches!(name.as_str(), "caption" | "col" | "colgroup") => {
                self.close_table_section();
                self.mode = Mode::InTable;
                StepResult::Reprocess(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
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
                self.clear_stack_back_to(&["tr"]);
                self.insert_element(&name, attrs);
                self.mode = Mode::InCell;
                self.insert_afe_marker();
                StepResult::Done
            }
            Token::EndTag { name } if name == "tr" => {
                if self.has_tag_in_scope("tr", &[]) {
                    self.pop_until_and_including("tr");
                }
                self.mode = Mode::InTableBody;
                StepResult::Done
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if matches!(
                name.as_str(),
                "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr"
            ) =>
            {
                if self.has_tag_in_scope("tr", &[]) {
                    self.pop_until_and_including("tr");
                    self.mode = Mode::InTableBody;
                }
                StepResult::Reprocess(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
            }
            Token::EndTag { name }
                if matches!(name.as_str(), "table" | "tbody" | "tfoot" | "thead") =>
            {
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
                    self.clear_afe_up_to_last_marker();
                }
                self.mode = Mode::InRow;
                StepResult::Done
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } if matches!(
                name.as_str(),
                "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr"
            ) =>
            {
                if self.has_tag_in_scope("td", &[]) || self.has_tag_in_scope("th", &[]) {
                    self.close_current_cell();
                }
                StepResult::Reprocess(Token::StartTag {
                    name,
                    attrs,
                    self_closing,
                })
            }
            Token::EndTag { name }
                if matches!(name.as_str(), "table" | "tbody" | "tfoot" | "thead" | "tr") =>
            {
                // Spec: ignore this end tag entirely unless an element
                // with that exact name is actually in table scope -- e.g.
                // a stray `</thead>` while only an implicit `<tbody>` is
                // open must not close the current cell at all.
                if !self.has_tag_in_table_scope(&name) {
                    return StepResult::Done;
                }
                if self.has_tag_in_scope("td", &[]) || self.has_tag_in_scope("th", &[]) {
                    self.close_current_cell();
                }
                StepResult::Reprocess(Token::EndTag { name })
            }
            other => self.step_in_body(other),
        }
    }

    fn step_after_body(&mut self, token: Token) -> StepResult {
        if let Token::Character(s) = &token {
            if s.trim().is_empty() {
                return self.step_in_body(token);
            }
            if let Some((ws, rest)) = Self::split_leading_whitespace(s) {
                let rest = rest.to_string();
                self.step_in_body(Token::Character(ws.to_string()));
                self.mode = Mode::InBody;
                return StepResult::Reprocess(Token::Character(rest));
            }
        }
        match &token {
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
        if let Token::Character(s) = &token {
            if s.trim().is_empty() {
                return self.step_in_body(token);
            }
            if let Some((ws, rest)) = Self::split_leading_whitespace(s) {
                let rest = rest.to_string();
                self.step_in_body(Token::Character(ws.to_string()));
                self.mode = Mode::InBody;
                return StepResult::Reprocess(Token::Character(rest));
            }
        }
        match &token {
            Token::Comment | Token::Doctype | Token::Eof => StepResult::Done,
            _ => {
                self.mode = Mode::InBody;
                StepResult::Reprocess(token)
            }
        }
    }
}

#[cfg(test)]
#[path = "tree_builder/tests.rs"]
mod tests;
