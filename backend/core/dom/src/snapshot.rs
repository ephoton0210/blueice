// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A document as bytes, for a process that holds its own copy of it.
//!
//! The encoding keeps what makes a copy *the same document*: every node's
//! `NodeId` and place in the tree (including the gaps left by removed nodes),
//! the next ID to allocate, and the file-control names. It does not keep the
//! change journal: a decoded document starts with an empty one.

use super::{Document, NodeData, NodeId, NodeRecord};
use std::collections::HashMap;

const MAGIC: &[u8; 5] = b"BDOC1";

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn opt(&mut self, v: Option<NodeId>) {
        match v {
            Some(id) => {
                self.u8(1);
                self.u64(id.as_u64());
            }
            None => self.u8(0),
        }
    }
    fn str(&mut self, v: &str) {
        self.u64(v.len() as u64);
        self.0.extend_from_slice(v.as_bytes());
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8)
            .map(|b| u64::from_le_bytes(b.try_into().expect("eight bytes")))
    }
    /// A length that cannot exceed what is left to read, so a corrupt count
    /// never allocates more than the input holds.
    fn len(&mut self) -> Option<usize> {
        let n = usize::try_from(self.u64()?).ok()?;
        (n <= self.0.len()).then_some(n)
    }
    fn opt(&mut self) -> Option<Option<NodeId>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(NodeId::from_u64(self.u64()?))),
            _ => None,
        }
    }
    fn str(&mut self) -> Option<String> {
        let n = self.len()?;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

impl Document {
    /// This document as bytes; see [`Document::decode`].
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(Vec::with_capacity(64 + self.nodes.len() * 48));
        w.0.extend_from_slice(MAGIC);
        w.u64(self.allocator.next);
        w.opt(self.root);
        let mut ids: Vec<NodeId> = self.nodes.keys().copied().collect();
        ids.sort_unstable();
        w.u64(ids.len() as u64);
        for id in ids {
            let record = &self.nodes[&id];
            w.u64(id.as_u64());
            w.opt(record.parent);
            w.opt(record.first_child);
            w.opt(record.last_child);
            w.opt(record.next_sibling);
            w.opt(record.prev_sibling);
            match &record.data {
                NodeData::Document => w.u8(0),
                NodeData::Element {
                    tag_name,
                    attributes,
                } => {
                    w.u8(1);
                    w.str(tag_name);
                    w.u64(attributes.len() as u64);
                    for (name, value) in attributes {
                        w.str(name);
                        w.str(value);
                    }
                }
                NodeData::Text { data } => {
                    w.u8(2);
                    w.str(data);
                }
            }
        }
        let mut named: Vec<(&NodeId, &Vec<String>)> = self.file_names.iter().collect();
        named.sort_unstable_by_key(|(id, _)| **id);
        w.u64(named.len() as u64);
        for (id, names) in named {
            w.u64(id.as_u64());
            w.u64(names.len() as u64);
            for name in names {
                w.str(name);
            }
        }
        w.0
    }

    /// A document from [`Document::encode`]'s bytes, or `None` when they are not a
    /// whole, consistent document (truncated, corrupt, or with a link to a node
    /// that is not there). The result has no change journal and a revision of 0.
    pub fn decode(bytes: &[u8]) -> Option<Document> {
        let mut r = Reader(bytes);
        if r.take(MAGIC.len())? != MAGIC {
            return None;
        }
        let next = r.u64()?;
        let root = r.opt()?;
        let count = r.len()?;
        let mut nodes = HashMap::with_capacity(count);
        for _ in 0..count {
            let id = NodeId::from_u64(r.u64()?);
            if id.as_u64() >= next {
                return None;
            }
            let parent = r.opt()?;
            let first_child = r.opt()?;
            let last_child = r.opt()?;
            let next_sibling = r.opt()?;
            let prev_sibling = r.opt()?;
            let data = match r.u8()? {
                0 => NodeData::Document,
                1 => {
                    let tag_name = r.str()?;
                    let n = r.len()?;
                    let mut attributes = Vec::with_capacity(n);
                    for _ in 0..n {
                        attributes.push((r.str()?, r.str()?));
                    }
                    NodeData::Element {
                        tag_name,
                        attributes,
                    }
                }
                2 => NodeData::Text { data: r.str()? },
                _ => return None,
            };
            let record = NodeRecord {
                data,
                parent,
                first_child,
                last_child,
                next_sibling,
                prev_sibling,
            };
            if nodes.insert(id, record).is_some() {
                return None;
            }
        }
        let mut file_names = HashMap::new();
        for _ in 0..r.len()? {
            let id = NodeId::from_u64(r.u64()?);
            let n = r.len()?;
            let mut names = Vec::with_capacity(n);
            for _ in 0..n {
                names.push(r.str()?);
            }
            if !nodes.contains_key(&id) {
                return None;
            }
            file_names.insert(id, names);
        }
        if !r.0.is_empty() {
            return None;
        }
        // Every link must land on a node that is there, and the root must be one.
        let known = |id: Option<NodeId>| id.is_none_or(|id| nodes.contains_key(&id));
        if !known(root) || root.is_none() {
            return None;
        }
        for record in nodes.values() {
            if !(known(record.parent)
                && known(record.first_child)
                && known(record.last_child)
                && known(record.next_sibling)
                && known(record.prev_sibling))
            {
                return None;
            }
        }
        Some(Document {
            allocator: super::NodeIdAllocator { next },
            nodes,
            root,
            file_names,
            journal: Default::default(),
            revision: 0,
            quirks: false,
        })
    }
}

impl super::Changes {
    /// These changes as bytes; see [`super::Changes::decode`].
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.u8(u8::from(self.overflow));
        let mut touched: Vec<_> = self.touched.iter().collect();
        touched.sort_unstable_by_key(|(id, _)| **id);
        w.u64(touched.len() as u64);
        for (id, before) in touched {
            w.u64(id.as_u64());
            match before {
                super::Before::Element {
                    tag_name,
                    attributes,
                } => {
                    w.u8(1);
                    w.str(tag_name);
                    w.u64(attributes.len() as u64);
                    for (name, value) in attributes {
                        w.str(name);
                        w.str(value);
                    }
                }
                super::Before::Text { data } => {
                    w.u8(2);
                    w.str(data);
                }
            }
        }
        w.u64(self.attached.len() as u64);
        for (parent, child) in &self.attached {
            w.u64(parent.as_u64());
            w.u64(child.as_u64());
        }
        w.u64(self.detached.len() as u64);
        for d in &self.detached {
            w.u64(d.parent.as_u64());
            w.u64(d.child.as_u64());
            w.opt(d.prev);
            w.opt(d.next);
        }
        w.0
    }

    /// Changes from [`super::Changes::encode`]'s bytes, or `None` when they are
    /// not whole.
    pub fn decode(bytes: &[u8]) -> Option<super::Changes> {
        let mut r = Reader(bytes);
        let overflow = match r.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let mut touched = HashMap::new();
        for _ in 0..r.len()? {
            let id = NodeId::from_u64(r.u64()?);
            let before = match r.u8()? {
                1 => {
                    let tag_name = r.str()?;
                    let n = r.len()?;
                    let mut attributes = Vec::with_capacity(n);
                    for _ in 0..n {
                        attributes.push((r.str()?, r.str()?));
                    }
                    super::Before::Element {
                        tag_name,
                        attributes,
                    }
                }
                2 => super::Before::Text { data: r.str()? },
                _ => return None,
            };
            touched.insert(id, before);
        }
        let n = r.len()?;
        let mut attached = Vec::with_capacity(n);
        for _ in 0..n {
            attached.push((NodeId::from_u64(r.u64()?), NodeId::from_u64(r.u64()?)));
        }
        let n = r.len()?;
        let mut detached = Vec::with_capacity(n);
        for _ in 0..n {
            detached.push(super::Detached {
                parent: NodeId::from_u64(r.u64()?),
                child: NodeId::from_u64(r.u64()?),
                prev: r.opt()?,
                next: r.opt()?,
            });
        }
        r.0.is_empty().then_some(super::Changes {
            touched,
            attached,
            detached,
            overflow,
        })
    }
}

impl Document {
    /// Adds `changes`, which happened after everything already in this
    /// document's journal, to it: a node keeps the data it had before its first
    /// touch, and an overflow on either side stays an overflow. For a document
    /// that was replaced by a copy that has since moved on, so the consumer of the
    /// journal sees what changed since it last looked across the replacement.
    pub fn absorb_changes(&mut self, changes: super::Changes) {
        if self.journal.overflow {
            return;
        }
        if changes.overflow {
            self.journal = super::Changes {
                overflow: true,
                ..Default::default()
            };
            return;
        }
        for (id, before) in changes.touched {
            self.journal.touched.entry(id).or_insert(before);
        }
        self.journal.attached.extend(changes.attached);
        self.journal.detached.extend(changes.detached);
        self.check_journal_limit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(tag: &str, attrs: &[(&str, &str)]) -> NodeData {
        NodeData::Element {
            tag_name: tag.to_string(),
            attributes: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    /// A document with nesting, text, attributes with awkward characters, a removed
    /// subtree (a gap in the ID space) and file-control names.
    fn sample() -> Document {
        let mut doc = Document::new();
        let root = doc.root();
        let html = doc.create_node(element("html", &[("lang", "zh-TW")]));
        doc.append_child(root, html);
        let body = doc.create_node(element("body", &[("class", "a \"b\" <c>"), ("data-x", "")]));
        doc.append_child(html, body);
        let doomed = doc.create_node(element("div", &[]));
        let inner = doc.create_node(NodeData::Text {
            data: "gone".into(),
        });
        doc.append_child(body, doomed);
        doc.append_child(doomed, inner);
        let p = doc.create_node(element("p", &[("id", "p1")]));
        doc.append_child(body, p);
        let t = doc.create_node(NodeData::Text {
            data: "你好 😀 world\n\ttabs".into(),
        });
        doc.append_child(p, t);
        let before = doc.create_node(element("hr", &[]));
        doc.insert_before(body, before, Some(p));
        doc.remove_subtree(doomed);
        let input = doc.create_node(element("input", &[("type", "file")]));
        doc.append_child(body, input);
        doc.set_file_control_names(input, vec!["a.txt".into(), "日本語.png".into()]);
        doc
    }

    #[test]
    fn a_decoded_document_is_the_same_document() {
        let doc = sample();
        let copy = Document::decode(&doc.encode()).expect("decodes");
        assert_eq!(crate::dump(&copy), crate::dump(&doc));
        assert_eq!(copy.root(), doc.root());
        assert_eq!(copy.next_node_id(), doc.next_node_id());
        // Same IDs, same places, same sibling links (not just the same text).
        let ids = |d: &Document| {
            let mut out = Vec::new();
            let mut stack = vec![d.root()];
            while let Some(id) = stack.pop() {
                out.push((
                    id,
                    d.parent(id),
                    d.first_child(id),
                    d.last_child(id),
                    d.next_sibling(id),
                    d.prev_sibling(id),
                ));
                stack.extend(d.children(id));
            }
            out.sort();
            out
        };
        assert_eq!(ids(&copy), ids(&doc));
        let input = doc
            .children(
                doc.children(doc.children(doc.root()).next().unwrap())
                    .next()
                    .unwrap(),
            )
            .last()
            .unwrap();
        assert_eq!(copy.file_control_names(input), ["a.txt", "日本語.png"]);
        assert_eq!(copy.data(input), doc.data(input));
    }

    #[test]
    fn a_decoded_document_goes_on_allocating_where_the_original_would() {
        let mut doc = sample();
        let mut copy = Document::decode(&doc.encode()).unwrap();
        assert_eq!(
            copy.create_node(NodeData::Text { data: "x".into() }),
            doc.create_node(NodeData::Text { data: "x".into() })
        );
    }

    #[test]
    fn a_decoded_document_has_no_journal_and_a_fresh_revision() {
        let doc = sample();
        let mut copy = Document::decode(&doc.encode()).unwrap();
        assert!(copy.take_changes().is_empty());
        assert_eq!(copy.revision(), 0);
        assert!(doc.revision() > 0);
    }

    #[test]
    fn every_mutation_moves_the_revision_and_reads_do_not() {
        let mut doc = Document::new();
        let root = doc.root();
        let mut last = doc.revision();
        let mut moved = |doc: &Document, what: &str| {
            assert!(doc.revision() > last, "{what} did not move the revision");
            last = doc.revision();
        };
        let a = doc.create_node(element("a", &[]));
        moved(&doc, "create_node");
        doc.append_child(root, a);
        moved(&doc, "append_child");
        let b = doc.create_node(element("b", &[]));
        moved(&doc, "create_node");
        doc.insert_before(root, b, Some(a));
        moved(&doc, "insert_before");
        let _ = doc.data_mut(a);
        moved(&doc, "data_mut");
        doc.set_file_control_names(a, vec!["f".into()]);
        moved(&doc, "set_file_control_names");
        doc.detach(b);
        moved(&doc, "detach");
        doc.remove_subtree(a);
        moved(&doc, "remove_subtree");
        let at = doc.revision();
        let _ = (
            doc.children(root).count(),
            doc.data(root),
            crate::dump(&doc),
            doc.encode(),
        );
        assert_eq!(doc.revision(), at, "reading changed the revision");
    }

    #[test]
    fn bytes_that_are_not_a_whole_document_are_refused_without_panicking() {
        let bytes = sample().encode();
        for cut in 0..bytes.len() {
            assert!(Document::decode(&bytes[..cut]).is_none(), "prefix {cut}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(Document::decode(&longer).is_none(), "trailing bytes");
        assert!(Document::decode(b"").is_none());
        assert!(Document::decode(b"not a document at all").is_none());
        // Flip every byte in turn: it may decode (a changed letter is still a
        // document) but must never panic or hang.
        for i in 0..bytes.len() {
            let mut corrupt = bytes.clone();
            corrupt[i] ^= 0xff;
            let _ = Document::decode(&corrupt);
        }
    }

    #[test]
    fn a_link_to_a_node_that_is_not_there_is_refused() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(element("a", &[]));
        doc.append_child(root, a);
        let mut bytes = doc.encode();
        // The last node's parent link is the root (id 0); point it at id 99.
        let needle = [1u8, 0, 0, 0, 0, 0, 0, 0, 0];
        let at = bytes
            .windows(needle.len())
            .rposition(|w| w == needle)
            .expect("a link to the root");
        bytes[at + 1] = 99;
        assert!(Document::decode(&bytes).is_none());
    }

    #[test]
    fn changes_round_trip_and_a_truncated_record_is_refused() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(element("a", &[("href", "/x")]));
        doc.append_child(root, a);
        let _ = doc.take_changes();
        if let NodeData::Element { attributes, .. } = doc.data_mut(a) {
            attributes.push(("class".into(), "k".into()));
        }
        let t = doc.create_node(NodeData::Text { data: "t".into() });
        doc.append_child(a, t);
        doc.detach(t);
        let changes = doc.take_changes();
        assert!(!changes.is_empty());
        let bytes = changes.encode();
        assert_eq!(super::super::Changes::decode(&bytes), Some(changes.clone()));
        for cut in 0..bytes.len() {
            assert!(super::super::Changes::decode(&bytes[..cut]).is_none(), "cut {cut}");
        }
        let overflowed = super::super::Changes {
            overflow: true,
            ..Default::default()
        };
        assert_eq!(
            super::super::Changes::decode(&overflowed.encode()),
            Some(overflowed)
        );
    }

    #[test]
    fn absorbed_changes_follow_the_ones_already_there_and_keep_the_earliest_before() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(element("a", &[("n", "0")]));
        doc.append_child(root, a);
        let _ = doc.take_changes();
        // A change the document already holds...
        if let NodeData::Element { attributes, .. } = doc.data_mut(a) {
            attributes[0].1 = "1".into();
        }
        // ...and a later one, made elsewhere, on the same node and a new one.
        let mut later = super::super::Changes::default();
        later.touched.insert(
            a,
            super::super::Before::Element {
                tag_name: "a".into(),
                attributes: vec![("n".into(), "1".into())],
            },
        );
        later.attached.push((root, NodeId::from_u64(99)));
        doc.absorb_changes(later);
        let held = doc.take_changes();
        assert_eq!(
            held.touched[&a],
            super::super::Before::Element {
                tag_name: "a".into(),
                attributes: vec![("n".into(), "0".into())],
            },
            "the data from before the first touch survives"
        );
        assert_eq!(held.attached, vec![(root, NodeId::from_u64(99))]);
        assert!(!held.overflow);

        // An overflow on either side is an overflow.
        doc.absorb_changes(super::super::Changes {
            overflow: true,
            ..Default::default()
        });
        assert!(doc.take_changes().overflow);
        let mut overflowed = Document::new();
        overflowed.absorb_changes(super::super::Changes {
            overflow: true,
            ..Default::default()
        });
        overflowed.absorb_changes(super::super::Changes {
            attached: vec![(root, root)],
            ..Default::default()
        });
        assert!(overflowed.take_changes().overflow);
    }
}
