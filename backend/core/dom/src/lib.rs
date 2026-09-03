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

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[&id].parent
    }

    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[&id].first_child
    }

    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[&id].next_sibling
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
