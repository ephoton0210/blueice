// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core DOM tree: node identity and tree structure.
//!
//! Every node gets a [`NodeId`] assigned eagerly at construction from a
//! monotonic counter, never reused. See
//! `development/browser_core/research/dom.md`: Blink's `DOMNodeId` is the
//! closest prior art, but assigns lazily on first request; BlueIce
//! assigns eagerly since every node needs a stable ID from creation
//! (plan §1), not just the ones that happen to need one later.

use std::collections::HashMap;

/// Stable identity for a DOM node, assigned once at construction and
/// never reused -- a monotonic counter, not a slab/array index, so a
/// freed node's ID can never be handed to an unrelated later node (the
/// ABA hazard a reused index would create).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u64);

impl NodeId {
    /// The raw counter value -- needed wherever a `NodeId` has to
    /// cross a boundary that can't carry the type itself, e.g.
    /// `phase-5-ai-representation-output/PLAN.md`'s `AiNode::id`,
    /// serialized over `blueice-ipc`'s wire protocol as a plain `u64`.
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Reconstructs a `NodeId` from a raw value previously obtained
    /// from [`NodeId::as_u64`] -- for turning a client-supplied
    /// integer (e.g. `phase-5-ai-representation-output/PLAN.md`'s
    /// `ClientMessage::ActOn`) back into something [`Document`]'s
    /// lookups accept. Round-tripping a value this crate itself
    /// allocated is always well-formed; a value that was never
    /// allocated (or belongs to a different `Document`) simply won't
    /// match anything -- callers should check [`Document::contains`]
    /// rather than assume every `NodeId` resolves to a live node.
    pub fn from_u64(id: u64) -> NodeId {
        NodeId(id)
    }
}

#[derive(Debug, Default)]
struct NodeIdAllocator {
    next: u64,
}

impl NodeIdAllocator {
    fn allocate(&mut self) -> NodeId {
        let id = NodeId(self.next);
        self.next += 1;
        id
    }
}

/// A node's own content, independent of its position in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeData {
    Document,
    Element {
        tag_name: String,
        attributes: Vec<(String, String)>,
    },
    Text {
        data: String,
    },
}

#[derive(Debug)]
struct NodeRecord {
    data: NodeData,
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    last_child: Option<NodeId>,
    next_sibling: Option<NodeId>,
    prev_sibling: Option<NodeId>,
}

/// A single document's DOM tree: owns every node and is the sole place
/// [`NodeId`]s are resolved to node data. Kept as a plain node table
/// (not `Rc<RefCell<..>>` cycles) so removed subtrees actually free
/// their memory rather than leaking on a reference cycle -- load-bearing
/// for `core`'s low-memory requirement (plan §1).
#[derive(Debug, Default)]
pub struct Document {
    allocator: NodeIdAllocator,
    nodes: HashMap<NodeId, NodeRecord>,
    root: Option<NodeId>,
}

impl Document {
    pub fn new() -> Self {
        let mut doc = Document::default();
        let root = doc.create_node(NodeData::Document);
        doc.root = Some(root);
        doc
    }

    pub fn root(&self) -> NodeId {
        self.root.expect("Document::new always creates a root")
    }

    /// Allocates a new node with a fresh, never-reused [`NodeId`]. The
    /// node starts detached; use [`Document::append_child`] to place it
    /// in the tree.
    pub fn create_node(&mut self, data: NodeData) -> NodeId {
        let id = self.allocator.allocate();
        self.nodes.insert(
            id,
            NodeRecord {
                data,
                parent: None,
                first_child: None,
                last_child: None,
                next_sibling: None,
                prev_sibling: None,
            },
        );
        id
    }

    pub fn data(&self, id: NodeId) -> &NodeData {
        &self.nodes[&id].data
    }

    /// Whether `id` resolves to a live node in this `Document` -- for
    /// callers that receive a `NodeId` from outside the type system's
    /// own guarantees (e.g. reconstructed via [`NodeId::from_u64`] from
    /// a client-supplied integer) and need to check before calling a
    /// panicking accessor like [`Document::data`].
    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Mutable access to `id`'s own data -- e.g. for
    /// `phase-5-ai-representation-output/PLAN.md`'s `NodeAction::SetValue`,
    /// which needs to update an `<input>`'s `value` attribute in place
    /// without detaching and recreating the node (that would mint a new
    /// `NodeId`, breaking plan §1's "stable ID across mutations"
    /// guarantee for the very node being mutated).
    pub fn data_mut(&mut self, id: NodeId) -> &mut NodeData {
        &mut self.nodes.get_mut(&id).expect("NodeId must belong to this Document").data
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[&id].parent
    }

    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[&id].first_child
    }

    /// The natural counterpart to [`Document::first_child`] -- added
    /// alongside [`Document::prev_sibling`] specifically so
    /// `blueice-html`'s tree builder can check "is the node I'm about
    /// to insert text next to already a Text node" in O(1) without
    /// walking the whole sibling list, per HTML5's "insert a
    /// character" algorithm (an adjacent Text node must be appended
    /// to, not duplicated as a new sibling).
    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[&id].last_child
    }

    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[&id].next_sibling
    }

    /// The natural counterpart to [`Document::next_sibling`] -- see
    /// [`Document::last_child`]'s docs for why this pair exists.
    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[&id].prev_sibling
    }

    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.first_child(id), move |&n| self.next_sibling(n))
    }

    /// Appends `child` as the last child of `parent`.
    ///
    /// # Panics
    /// Panics if `child` is already attached somewhere in the tree --
    /// call [`Document::detach`] first.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        assert!(
            self.nodes[&child].parent.is_none(),
            "child is already attached; detach it first"
        );

        let prev_last = self.nodes[&parent].last_child;
        if let Some(prev_last) = prev_last {
            self.nodes.get_mut(&prev_last).unwrap().next_sibling = Some(child);
        } else {
            self.nodes.get_mut(&parent).unwrap().first_child = Some(child);
        }

        let child_rec = self.nodes.get_mut(&child).unwrap();
        child_rec.parent = Some(parent);
        child_rec.prev_sibling = prev_last;

        self.nodes.get_mut(&parent).unwrap().last_child = Some(child);
    }

    /// Inserts `child` into `parent`'s children, immediately before
    /// `reference` -- or as the last child if `reference` is `None`.
    /// Needed by the HTML parser's foster-parenting error recovery
    /// (`../html/src/tree_builder.rs`), which must place misplaced
    /// table content as a sibling *before* an already-inserted `<table>`
    /// rather than appending it.
    ///
    /// # Panics
    /// Panics if `child` is already attached, or if `reference` is not
    /// currently a child of `parent`.
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) {
        let Some(reference) = reference else {
            return self.append_child(parent, child);
        };
        assert!(
            self.nodes[&child].parent.is_none(),
            "child is already attached; detach it first"
        );
        assert_eq!(
            self.nodes[&reference].parent,
            Some(parent),
            "reference must be a child of parent"
        );

        let prev = self.nodes[&reference].prev_sibling;
        if let Some(prev) = prev {
            self.nodes.get_mut(&prev).unwrap().next_sibling = Some(child);
        } else {
            self.nodes.get_mut(&parent).unwrap().first_child = Some(child);
        }
        self.nodes.get_mut(&reference).unwrap().prev_sibling = Some(child);

        let child_rec = self.nodes.get_mut(&child).unwrap();
        child_rec.parent = Some(parent);
        child_rec.prev_sibling = prev;
        child_rec.next_sibling = Some(reference);
    }

    /// Detaches `id` from its parent/siblings, but keeps it (and its
    /// subtree) resolvable in the node table. Use
    /// [`Document::remove_subtree`] to actually reclaim memory.
    pub fn detach(&mut self, id: NodeId) {
        let (parent, prev, next) = {
            let rec = &self.nodes[&id];
            (rec.parent, rec.prev_sibling, rec.next_sibling)
        };

        if let Some(prev) = prev {
            self.nodes.get_mut(&prev).unwrap().next_sibling = next;
        } else if let Some(parent) = parent {
            self.nodes.get_mut(&parent).unwrap().first_child = next;
        }

        if let Some(next) = next {
            self.nodes.get_mut(&next).unwrap().prev_sibling = prev;
        } else if let Some(parent) = parent {
            self.nodes.get_mut(&parent).unwrap().last_child = prev;
        }

        let rec = self.nodes.get_mut(&id).unwrap();
        rec.parent = None;
        rec.prev_sibling = None;
        rec.next_sibling = None;
    }

    /// Detaches `id` (if attached) and frees it and its entire subtree
    /// from the node table, reclaiming their memory. `id` and every
    /// descendant [`NodeId`] are invalid to use after this call.
    pub fn remove_subtree(&mut self, id: NodeId) {
        self.detach(id);
        self.free_subtree(id);
    }

    fn free_subtree(&mut self, id: NodeId) {
        let mut child = self.nodes.get(&id).and_then(|r| r.first_child);
        while let Some(current) = child {
            child = self.nodes[&current].next_sibling;
            self.free_subtree(current);
        }
        self.nodes.remove(&id);
    }
}

/// Renders `doc` as a canonical, whitespace-exact tree dump -- a
/// `| `-prefixed, 2-space-per-depth indented listing, elements as
/// `<tag>`, attributes as their own sorted, one-level-deeper lines,
/// text as `"content"`. Adapted from the html5lib-tests tree-
/// construction format rather than invented, because it's already the
/// de facto standard this exact kind of dump takes in every browser
/// engine this project reads as reference, and it's trivially
/// diffable by a human. Two documents with the same effective shape
/// always produce byte-identical dumps (attributes are sorted, so
/// insertion order never leaks into the comparison) -- what makes this
/// usable both as `blueice-testing`'s fixture format (`#document`
/// sections; re-exported from there as `dump_dom` for that corpus's
/// existing call sites) and, per
/// `development/browser_core/testing/TEST_PLAN.md`'s Chromium
/// differential-testing plan, as a `blueice_ipc::ClientMessage::GetDom`
/// reply a comparison harness can diff against a serialized Chromium
/// DOM. Comments/doctypes never appear, matching `blueice_html`'s tree
/// builder not materializing them as nodes.
pub fn dump(doc: &Document) -> String {
    let mut out = String::new();
    for child in doc.children(doc.root()) {
        dump_node(doc, child, 0, &mut out);
    }
    out
}

fn dump_node(doc: &Document, id: NodeId, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    match doc.data(id) {
        NodeData::Document => unreachable!("the Document node is never its own child"),
        NodeData::Element { tag_name, attributes } => {
            out.push_str(&format!("| {indent}<{tag_name}>\n"));
            let mut attrs: Vec<_> = attributes.iter().collect();
            attrs.sort_by(|a, b| a.0.cmp(&b.0));
            let attr_indent = "  ".repeat(depth + 1);
            for (name, value) in attrs {
                out.push_str(&format!("| {attr_indent}{name}=\"{value}\"\n"));
            }
            for child in doc.children(id) {
                dump_node(doc, child, depth + 1, out);
            }
        }
        NodeData::Text { data } => {
            out.push_str(&format!("| {indent}\"{data}\"\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_renders_nested_elements_and_text() {
        let mut doc = Document::new();
        let root = doc.root();
        let p = doc.create_node(NodeData::Element { tag_name: "p".to_string(), attributes: vec![] });
        doc.append_child(root, p);
        let text = doc.create_node(NodeData::Text { data: "hi".to_string() });
        doc.append_child(p, text);

        assert_eq!(dump(&doc), "| <p>\n|   \"hi\"\n");
    }

    #[test]
    fn dump_sorts_attributes_regardless_of_insertion_order() {
        let mut doc = Document::new();
        let root = doc.root();
        let div = doc.create_node(NodeData::Element {
            tag_name: "div".to_string(),
            attributes: vec![("class".to_string(), "b".to_string()), ("id".to_string(), "a".to_string())],
        });
        doc.append_child(root, div);

        assert_eq!(dump(&doc), "| <div>\n|   class=\"b\"\n|   id=\"a\"\n");
    }

    #[test]
    fn dump_of_empty_document_is_empty_string() {
        let doc = Document::new();
        assert_eq!(dump(&doc), "");
    }

    fn elem(tag: &str) -> NodeData {
        NodeData::Element {
            tag_name: tag.to_string(),
            attributes: vec![],
        }
    }

    fn text(data: &str) -> NodeData {
        NodeData::Text {
            data: data.to_string(),
        }
    }

    #[test]
    fn data_mut_allows_updating_an_elements_attributes_in_place() {
        let mut doc = Document::new();
        let input = doc.create_node(NodeData::Element { tag_name: "input".to_string(), attributes: vec![("type".to_string(), "text".to_string())] });
        doc.append_child(doc.root(), input);

        if let NodeData::Element { attributes, .. } = doc.data_mut(input) {
            attributes.push(("value".to_string(), "hello".to_string()));
        }

        assert_eq!(doc.data(input), &NodeData::Element { tag_name: "input".to_string(), attributes: vec![("type".to_string(), "text".to_string()), ("value".to_string(), "hello".to_string())] });
    }

    #[test]
    fn data_mut_does_not_change_the_nodes_id_or_tree_position() {
        let mut doc = Document::new();
        let root = doc.root();
        let node = doc.create_node(elem("div"));
        doc.append_child(root, node);

        if let NodeData::Element { tag_name, .. } = doc.data_mut(node) {
            *tag_name = "span".to_string();
        }

        assert_eq!(doc.parent(node), Some(root), "mutating data must not detach the node");
        assert_eq!(doc.data(node), &elem("span"));
    }

    #[test]
    fn node_id_round_trips_through_as_u64_and_from_u64() {
        let mut doc = Document::new();
        let node = doc.create_node(elem("div"));
        let raw = node.as_u64();
        assert_eq!(NodeId::from_u64(raw), node);
    }

    #[test]
    fn contains_is_true_for_a_real_node_and_false_for_an_unallocated_id() {
        let mut doc = Document::new();
        let node = doc.create_node(elem("div"));
        assert!(doc.contains(node));
        assert!(!doc.contains(NodeId::from_u64(999_999)));
    }

    #[test]
    fn node_ids_are_never_reused() {
        let mut doc = Document::new();
        let a = doc.create_node(elem("div"));
        doc.append_child(doc.root(), a);
        doc.remove_subtree(a);
        let b = doc.create_node(elem("div"));
        assert_ne!(a, b, "NodeId must not be reused after removal (ABA hazard)");
    }

    #[test]
    fn append_and_iterate_children() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(text("a"));
        let b = doc.create_node(text("b"));
        doc.append_child(root, a);
        doc.append_child(root, b);
        let kids: Vec<_> = doc.children(root).collect();
        assert_eq!(kids, vec![a, b]);
    }

    #[test]
    fn last_child_and_prev_sibling_are_the_natural_counterparts_of_first_child_and_next_sibling() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(text("a"));
        let b = doc.create_node(text("b"));
        doc.append_child(root, a);
        doc.append_child(root, b);

        assert_eq!(doc.last_child(root), Some(b));
        assert_eq!(doc.prev_sibling(b), Some(a));
        assert_eq!(doc.prev_sibling(a), None);
    }

    #[test]
    fn last_child_is_none_for_a_childless_node() {
        let doc = Document::new();
        assert_eq!(doc.last_child(doc.root()), None);
    }

    #[test]
    fn detach_removes_from_tree_but_keeps_node_resolvable() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(text("a"));
        doc.append_child(root, a);
        doc.detach(a);
        assert_eq!(doc.children(root).count(), 0);
        assert_eq!(doc.data(a), &text("a"));
        assert_eq!(doc.parent(a), None);
    }

    #[test]
    fn remove_subtree_removes_descendants_too() {
        let mut doc = Document::new();
        let root = doc.root();
        let parent = doc.create_node(elem("div"));
        let child = doc.create_node(text("x"));
        doc.append_child(root, parent);
        doc.append_child(parent, child);
        doc.remove_subtree(parent);
        assert_eq!(doc.children(root).count(), 0);
    }

    #[test]
    #[should_panic(expected = "already attached")]
    fn append_child_twice_panics() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(text("a"));
        doc.append_child(root, a);
        doc.append_child(root, a);
    }

    #[test]
    fn insert_before_places_child_ahead_of_reference() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(text("a"));
        let b = doc.create_node(text("b"));
        doc.append_child(root, a);
        let c = doc.create_node(text("c"));
        doc.insert_before(root, b, Some(a));
        doc.insert_before(root, c, Some(a));
        let kids: Vec<_> = doc.children(root).collect();
        assert_eq!(kids, vec![b, c, a]);
    }

    #[test]
    fn insert_before_none_reference_appends() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(text("a"));
        let b = doc.create_node(text("b"));
        doc.append_child(root, a);
        doc.insert_before(root, b, None);
        let kids: Vec<_> = doc.children(root).collect();
        assert_eq!(kids, vec![a, b]);
    }

    #[test]
    fn insert_before_updates_prev_and_next_sibling_links() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(text("a"));
        let c = doc.create_node(text("c"));
        doc.append_child(root, a);
        doc.append_child(root, c);
        let b = doc.create_node(text("b"));
        doc.insert_before(root, b, Some(c));
        assert_eq!(doc.next_sibling(a), Some(b));
        assert_eq!(doc.next_sibling(b), Some(c));
        let kids: Vec<_> = doc.children(root).collect();
        assert_eq!(kids, vec![a, b, c]);
    }

    #[test]
    #[should_panic(expected = "already attached")]
    fn insert_before_already_attached_child_panics() {
        let mut doc = Document::new();
        let root = doc.root();
        let a = doc.create_node(text("a"));
        doc.append_child(root, a);
        doc.insert_before(root, a, None);
    }

    #[test]
    #[should_panic(expected = "reference must be a child of parent")]
    fn insert_before_reference_not_a_child_panics() {
        let mut doc = Document::new();
        let root = doc.root();
        let other_parent = doc.create_node(elem("div"));
        doc.append_child(root, other_parent);
        let reference = doc.create_node(text("r"));
        doc.append_child(other_parent, reference);
        let child = doc.create_node(text("x"));
        doc.insert_before(root, child, Some(reference));
    }
}
