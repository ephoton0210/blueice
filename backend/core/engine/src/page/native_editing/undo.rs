// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private, bounded text transactions. Only availability leaves the core.
use super::{ControlInfo, EditorSession};
use blueice_dom::NodeId;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_TRANSACTIONS: usize = 128;

#[derive(Clone)]
pub(super) struct Value {
    text: Zeroizing<String>,
    anchor: u32,
    cursor: u32,
}
impl Value {
    pub(super) fn from_editor(editor: &EditorSession) -> Self {
        Self {
            text: Zeroizing::new(editor.observed.clone()),
            anchor: editor.anchor,
            cursor: editor.cursor,
        }
    }
    pub(super) fn from_composition(editor: &EditorSession) -> Self {
        let composition = editor.composition.as_ref().expect("live composition");
        Self {
            text: Zeroizing::new(composition.original.clone()),
            anchor: composition.anchor,
            cursor: composition.cursor,
        }
    }
    pub(super) fn restore(self, editor: &mut EditorSession) {
        editor.observed = self.text.to_string();
        editor.anchor = self.anchor;
        editor.cursor = self.cursor;
        editor.composition = None;
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Atomic,
    Typing,
    DeleteBackward,
    DeleteForward,
}
struct Transaction {
    before: Value,
    after: Value,
    sequence: u64,
}
impl Transaction {
    fn bytes(&self) -> usize {
        self.before.text.len() + self.after.text.len()
    }
}
struct ControlHistory {
    undo: VecDeque<Transaction>,
    redo: Vec<Transaction>,
    protected: bool,
    multiline: bool,
    group: Option<(Kind, Instant, u64)>,
}
impl ControlHistory {
    fn matches(&self, value: &str, info: ControlInfo) -> bool {
        self.protected == info.protected
            && self.multiline == info.multiline
            && self
                .undo
                .back()
                .map(|t| t.after.text.as_str())
                .or_else(|| self.redo.last().map(|t| t.before.text.as_str()))
                == Some(value)
    }
}
#[derive(Default)]
pub(in crate::page) struct UndoHistory {
    controls: HashMap<NodeId, ControlHistory>,
    sequence: u64,
    pub(super) limited: bool,
}
impl UndoHistory {
    pub(in crate::page) fn clear(&mut self) {
        self.controls.clear();
        self.limited = false;
    }
    pub(in crate::page) fn forget(&mut self, node: NodeId) {
        self.controls.remove(&node);
    }
    pub(in crate::page) fn prune(&mut self, doc: &blueice_dom::Document) {
        self.controls.retain(|node, history| {
            super::control_info(doc, *node).is_some_and(|info| {
                info.writable
                    && info.protected == history.protected
                    && info.multiline == history.multiline
            })
        });
    }
    pub(super) fn close_group(&mut self, node: NodeId) {
        if let Some(history) = self.controls.get_mut(&node) {
            history.group = None;
        }
    }
    pub(super) fn availability(&self, editor: &EditorSession, info: ControlInfo) -> (bool, bool) {
        if !info.writable || editor.composition.is_some() {
            return (false, false);
        }
        self.controls
            .get(&editor.node)
            .filter(|h| h.matches(&editor.observed, info))
            .map(|h| (!h.undo.is_empty(), !h.redo.is_empty()))
            .unwrap_or_default()
    }
    pub(super) fn record(
        &mut self,
        node: NodeId,
        info: ControlInfo,
        before: Value,
        after: Value,
        kind: Kind,
        focus: u64,
    ) {
        if before.text == after.text {
            self.close_group(node);
            return;
        }
        if self
            .controls
            .get(&node)
            .is_some_and(|h| !h.matches(&before.text, info))
        {
            self.forget(node);
        }
        let history = self.controls.entry(node).or_insert_with(|| ControlHistory {
            undo: VecDeque::new(),
            redo: Vec::new(),
            protected: info.protected,
            multiline: info.multiline,
            group: None,
        });
        let now = Instant::now();
        let coalesce = kind != Kind::Atomic
            && history.redo.is_empty()
            && history.group.is_some_and(|(old, at, generation)| {
                old == kind
                    && generation == focus
                    && now.duration_since(at) < Duration::from_secs(1)
            })
            && history.undo.back().is_some_and(|t| {
                t.after.anchor == before.anchor && t.after.cursor == before.cursor
            });
        history.redo.clear();
        self.sequence = self.sequence.wrapping_add(1);
        if coalesce {
            let transaction = history.undo.back_mut().expect("coalescing transaction");
            transaction.after = after;
        } else {
            history.undo.push_back(Transaction {
                before,
                after,
                sequence: self.sequence,
            });
        }
        history.group = (kind != Kind::Atomic).then_some((kind, now, focus));
        self.enforce_limits();
    }
    pub(super) fn replay(
        &mut self,
        editor: &mut EditorSession,
        info: ControlInfo,
        redo: bool,
    ) -> bool {
        if !info.writable || editor.composition.is_some() {
            return false;
        }
        if self
            .controls
            .get(&editor.node)
            .is_some_and(|h| !h.matches(&editor.observed, info))
        {
            self.forget(editor.node);
        }
        let Some(history) = self.controls.get_mut(&editor.node) else {
            return false;
        };
        history.group = None;
        if redo {
            let Some(transaction) = history.redo.pop() else {
                return false;
            };
            transaction.after.clone().restore(editor);
            history.undo.push_back(transaction);
        } else {
            let Some(transaction) = history.undo.pop_back() else {
                return false;
            };
            transaction.before.clone().restore(editor);
            history.redo.push(transaction);
        }
        true
    }
    fn enforce_limits(&mut self) {
        loop {
            let mut bytes = 0;
            let mut count = 0;
            let mut oldest = None;
            for (node, history) in &self.controls {
                for transaction in history.undo.iter().chain(&history.redo) {
                    bytes += transaction.bytes();
                    count += 1;
                }
                for (transaction, redo) in
                    [(history.undo.front(), false), (history.redo.last(), true)]
                {
                    if let Some(transaction) = transaction {
                        if oldest.is_none_or(|(_, _, sequence)| transaction.sequence < sequence) {
                            oldest = Some((*node, redo, transaction.sequence));
                        }
                    }
                }
            }
            if bytes <= MAX_BYTES && count <= MAX_TRANSACTIONS {
                break;
            }
            let (node, redo, _) = oldest.expect("bounded history contains a transaction");
            let history = self.controls.get_mut(&node).expect("bounded control");
            if redo {
                history.redo.clear();
            } else {
                history.undo.pop_front();
            }
            history.group = None;
            self.limited = true;
            if history.undo.is_empty() && history.redo.is_empty() {
                self.forget(node);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_budget_evicts_old_transactions_and_preserves_a_replayable_tail() {
        let mut doc = blueice_dom::Document::new();
        let node = doc.create_node(blueice_dom::NodeData::Element {
            tag_name: "input".into(),
            attributes: Vec::new(),
        });
        let info = ControlInfo {
            protected: true,
            multiline: false,
            writable: true,
        };
        let mut history = UndoHistory::default();
        let value = |c: char| Value {
            text: Zeroizing::new(c.to_string().repeat(65_536)),
            anchor: 65_536,
            cursor: 65_536,
        };
        for i in 0..40 {
            history.record(
                node,
                info,
                value(char::from(b'A' + i)),
                value(char::from(b'B' + i)),
                Kind::Atomic,
                1,
            );
        }
        assert!(history.limited);
        assert_eq!(history.controls[&node].undo.len(), 32);
        let mut editor = EditorSession {
            node,
            anchor: 65_536,
            cursor: 65_536,
            observed: char::from(b'A' + 40).to_string().repeat(65_536),
            composition: None,
        };
        for _ in 0..32 {
            assert!(history.replay(&mut editor, info, false));
        }
        assert!(!history.replay(&mut editor, info, false));
        assert_eq!(
            editor.observed,
            char::from(b'A' + 8).to_string().repeat(65_536)
        );
        for _ in 0..32 {
            assert!(history.replay(&mut editor, info, true));
        }
        assert_eq!(
            editor.observed,
            char::from(b'A' + 40).to_string().repeat(65_536)
        );
        history.clear();
        assert!(history.controls.is_empty());
        assert!(!history.limited);
    }

    #[test]
    fn global_budget_discards_an_old_redo_branch_without_breaking_new_controls() {
        let mut doc = blueice_dom::Document::new();
        let first = doc.create_node(blueice_dom::NodeData::Element {
            tag_name: "input".into(),
            attributes: Vec::new(),
        });
        let second = doc.create_node(blueice_dom::NodeData::Element {
            tag_name: "input".into(),
            attributes: Vec::new(),
        });
        let info = ControlInfo {
            protected: false,
            multiline: false,
            writable: true,
        };
        let value = |text: String| Value {
            text: Zeroizing::new(text),
            anchor: 0,
            cursor: 0,
        };
        let mut history = UndoHistory::default();
        for i in 0..4 {
            history.record(
                first,
                info,
                value(i.to_string()),
                value((i + 1).to_string()),
                Kind::Atomic,
                1,
            );
        }
        let mut editor = EditorSession {
            node: first,
            anchor: 0,
            cursor: 0,
            observed: "4".into(),
            composition: None,
        };
        for _ in 0..4 {
            assert!(history.replay(&mut editor, info, false));
        }
        for i in 0..128 {
            history.record(
                second,
                info,
                value(i.to_string()),
                value((i + 1).to_string()),
                Kind::Atomic,
                2,
            );
        }
        assert!(history.limited);
        assert!(!history.replay(&mut editor, info, true));
        assert_eq!(history.controls[&second].undo.len(), 128);
    }
}
