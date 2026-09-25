// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Live-translation document model (`phase-7-local-ai/PLAN.md`, "Assistant
//! design decisions" and checklist step T1). Pure functions over a
//! [`Document`]: which text nodes are translatable, substituting a
//! translation for them, and swapping back to the retained original.
//!
//! Translation is applied to the DOM right after parsing and before the
//! cascade, so layout always measures the text that is actually shown. The
//! original text of every substituted node is kept beside the document
//! (never discarded), so the person can toggle back, and so the
//! representation can still offer it. The gatekeeper never sees a translation:
//! it reviews the original HTML before this runs.
//!
//! The eligible list is deterministic for a given document, which is what lets
//! `core`'s navigation thread translate the batches from one parse while the
//! main thread substitutes them by ordinal into its own parse of the same HTML.

use blueice_dom::{Document, NodeData, NodeId};
use blueice_ipc::assistant::MAX_TRANSLATE_ITEM_BYTES;
use std::collections::BTreeMap;

/// Elements whose text is never shown as page prose (or must not be
/// rewritten: form text a person is editing).
const SKIPPED_ANCESTORS: &[&str] = &[
    "head", "script", "style", "noscript", "template", "textarea",
];

/// One translatable text node: its ID and the text to send (whitespace-trimmed;
/// the surrounding whitespace is restored when substituting).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslatableText {
    pub node: NodeId,
    pub text: String,
}

/// The document's translatable text nodes in document order. A node is
/// eligible when it has non-whitespace text of at most
/// [`MAX_TRANSLATE_ITEM_BYTES`] and no skipped ancestor.
pub fn translatable_texts(doc: &Document) -> Vec<TranslatableText> {
    let mut out = Vec::new();
    collect(doc, doc.root(), &mut out);
    out
}

fn collect(doc: &Document, id: NodeId, out: &mut Vec<TranslatableText>) {
    for child in doc.children(id) {
        match doc.data(child) {
            NodeData::Text { data } => {
                let trimmed = data.trim();
                if !trimmed.is_empty() && trimmed.len() <= MAX_TRANSLATE_ITEM_BYTES {
                    out.push(TranslatableText {
                        node: child,
                        text: trimmed.to_string(),
                    });
                }
            }
            NodeData::Element { tag_name, .. } => {
                if !SKIPPED_ANCESTORS
                    .iter()
                    .any(|skipped| tag_name.eq_ignore_ascii_case(skipped))
                {
                    collect(doc, child, out);
                }
            }
            NodeData::Document => collect(doc, child, out),
        }
    }
}

/// The substituted nodes of one document, so translation can be reversed.
#[derive(Debug, Default, Clone)]
pub struct TranslationState {
    originals: BTreeMap<NodeId, String>,
    translated: BTreeMap<NodeId, String>,
    shown: bool,
}

impl TranslationState {
    /// Whether the document currently shows the translation.
    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// True once any node was substituted (so a toggle means something).
    pub fn has_translation(&self) -> bool {
        !self.translated.is_empty()
    }

    /// The original text of `node` when it was substituted and is currently
    /// showing its translation. A node the page later rewrote itself is not
    /// reported: the original no longer describes what is on screen.
    pub fn original_of(&self, doc: &Document, node: NodeId) -> Option<&str> {
        if !self.shown || !doc.contains(node) {
            return None;
        }
        let NodeData::Text { data } = doc.data(node) else {
            return None;
        };
        (self.translated.get(&node)? == data).then(|| self.originals[&node].as_str())
    }
}

/// Substitutes `translations` (one per entry of [`translatable_texts`], same
/// order) into `doc`. Returns the state to keep, or `None` when the count does
/// not match, so a stale or wrong-length answer degrades to the original page
/// instead of misaligning text. An empty translation of non-empty text keeps
/// that node's original.
pub fn apply_translation(doc: &mut Document, translations: &[String]) -> Option<TranslationState> {
    let texts = translatable_texts(doc);
    if texts.len() != translations.len() {
        return None;
    }
    let mut state = TranslationState {
        shown: true,
        ..TranslationState::default()
    };
    for (source, translation) in texts.iter().zip(translations) {
        let translation = translation.trim();
        if translation.is_empty() {
            continue;
        }
        let NodeData::Text { data } = doc.data_mut(source.node) else {
            continue;
        };
        let original = data.clone();
        let leading = &original[..original.len() - original.trim_start().len()];
        let trailing = &original[original.trim_end().len()..];
        let shown = format!("{leading}{translation}{trailing}");
        if shown == original {
            continue;
        }
        *data = shown.clone();
        state.originals.insert(source.node, original);
        state.translated.insert(source.node, shown);
    }
    Some(state)
}

/// Swaps every substituted node to its original (`show == false`) or back to
/// its translation (`show == true`). Returns whether the document changed.
/// A node whose text the page has since rewritten is left alone.
pub fn set_shown(doc: &mut Document, state: &mut TranslationState, show: bool) -> bool {
    if state.shown == show {
        return false;
    }
    let (from, to) = if show {
        (&state.originals, &state.translated)
    } else {
        (&state.translated, &state.originals)
    };
    let mut changed = false;
    for (node, expected) in from {
        if !doc.contains(*node) {
            continue;
        }
        if let NodeData::Text { data } = doc.data_mut(*node) {
            if data == expected {
                *data = to[node].clone();
                changed = true;
            }
        }
    }
    state.shown = show;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(doc: &Document, node: NodeId) -> String {
        match doc.data(node) {
            NodeData::Text { data } => data.clone(),
            _ => panic!("not a text node"),
        }
    }

    fn doc(html: &str) -> Document {
        blueice_html::parse(html)
    }

    fn sources(doc: &Document) -> Vec<String> {
        translatable_texts(doc)
            .into_iter()
            .map(|t| t.text)
            .collect()
    }

    #[test]
    fn prose_is_extracted_in_document_order_and_trimmed() {
        let d = doc("<h1> Title </h1><p>One <b>two</b> three</p>");
        assert_eq!(sources(&d), ["Title", "One", "two", "three"]);
    }

    #[test]
    fn non_prose_and_form_text_are_skipped() {
        let d = doc("<head><title>T</title><style>p{}</style></head>\
             <body><script>var a=1</script><noscript>no</noscript>\
             <template><p>tpl</p></template><textarea>draft</textarea>\
             <p>kept</p></body>");
        assert_eq!(sources(&d), ["kept"]);
    }

    #[test]
    fn whitespace_only_and_oversized_text_are_skipped() {
        let big = "a".repeat(MAX_TRANSLATE_ITEM_BYTES + 1);
        let d = doc(&format!("<p>   </p><p>{big}</p><p>ok</p>"));
        assert_eq!(sources(&d), ["ok"]);
    }

    #[test]
    fn extraction_is_deterministic_across_parses() {
        let html = "<div>a<p>b</p>c</div><p>d</p>";
        assert_eq!(sources(&doc(html)), sources(&doc(html)));
    }

    #[test]
    fn applying_substitutes_text_and_keeps_surrounding_whitespace() {
        let mut d = doc("<p>Hello <b>big</b> world</p>");
        let state =
            apply_translation(&mut d, &["你好".into(), "大".into(), "世界".into()]).unwrap();
        let texts = translatable_texts(&d);
        let now: Vec<_> = texts.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(now, ["你好", "大", "世界"]);
        // Whitespace around each text node is exactly what the source had.
        assert_eq!(text_of(&d, texts[0].node), "你好 ");
        assert_eq!(text_of(&d, texts[2].node), " 世界");
        assert!(state.is_shown());
        assert!(state.has_translation());
    }

    #[test]
    fn a_wrong_length_answer_changes_nothing() {
        let mut d = doc("<p>a</p><p>b</p>");
        let before = blueice_dom::dump(&d);
        assert!(apply_translation(&mut d, &["x".into()]).is_none());
        assert_eq!(blueice_dom::dump(&d), before);
    }

    #[test]
    fn an_empty_or_identical_translation_keeps_that_node_original() {
        let mut d = doc("<p>a</p><p>b</p><p>c</p>");
        let state = apply_translation(&mut d, &["A".into(), "  ".into(), "c".into()]).unwrap();
        assert_eq!(sources(&d), ["A", "b", "c"]);
        // Only the node that actually changed is retained.
        assert_eq!(state.originals.len(), 1);
    }

    #[test]
    fn nothing_substituted_means_nothing_to_toggle() {
        let mut d = doc("<p>a</p>");
        let state = apply_translation(&mut d, &["a".into()]).unwrap();
        assert!(!state.has_translation());
    }

    #[test]
    fn toggling_restores_and_reapplies_the_translation() {
        let mut d = doc("<p>Hello</p><p>World</p>");
        let mut state = apply_translation(&mut d, &["你好".into(), "世界".into()]).unwrap();
        assert!(set_shown(&mut d, &mut state, false));
        assert_eq!(sources(&d), ["Hello", "World"]);
        assert!(!state.is_shown());
        assert!(!set_shown(&mut d, &mut state, false), "already original");
        assert!(set_shown(&mut d, &mut state, true));
        assert_eq!(sources(&d), ["你好", "世界"]);
    }

    #[test]
    fn a_node_the_page_rewrote_is_neither_clobbered_nor_reported() {
        let mut d = doc("<p>Hello</p><p>World</p>");
        let mut state = apply_translation(&mut d, &["你好".into(), "世界".into()]).unwrap();
        let nodes: Vec<_> = translatable_texts(&d).into_iter().map(|t| t.node).collect();
        if let NodeData::Text { data } = d.data_mut(nodes[0]) {
            *data = "script wrote this".to_string();
        }
        assert_eq!(state.original_of(&d, nodes[0]), None);
        assert_eq!(state.original_of(&d, nodes[1]), Some("World"));
        set_shown(&mut d, &mut state, false);
        assert_eq!(text_of(&d, nodes[0]), "script wrote this");
        assert_eq!(text_of(&d, nodes[1]), "World");
    }

    #[test]
    fn the_original_is_only_offered_while_the_translation_is_shown() {
        let mut d = doc("<p>Hello</p>");
        let mut state = apply_translation(&mut d, &["你好".into()]).unwrap();
        let node = translatable_texts(&d)[0].node;
        assert_eq!(state.original_of(&d, node), Some("Hello"));
        set_shown(&mut d, &mut state, false);
        assert_eq!(state.original_of(&d, node), None);
        // Unknown and non-text nodes have no original.
        assert_eq!(state.original_of(&d, d.root()), None);
        assert_eq!(TranslationState::default().original_of(&d, node), None);
    }

    #[test]
    fn a_removed_node_is_ignored_by_toggle() {
        let mut d = doc("<p>Hello</p>");
        let mut state = apply_translation(&mut d, &["你好".into()]).unwrap();
        let node = translatable_texts(&d)[0].node;
        d.remove_subtree(node);
        assert!(!set_shown(&mut d, &mut state, false));
        assert_eq!(state.original_of(&d, node), None);
    }
}
