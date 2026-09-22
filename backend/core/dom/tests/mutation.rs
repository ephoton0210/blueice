// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Integration tests for `blueice_dom::Document`, driven only through its
//! public API (never `pub(crate)`/private internals) -- the same
//! "exercise the crate's own public entry point" principle
//! `blueice-html`'s `tests/parsing.rs` and `blueice-css`'s
//! `tests/fixtures.rs` already follow, applied to the DOM crate itself.
//! `src/lib.rs`'s own `#[cfg(test)]` module already covers each
//! operation in isolation; this file targets combinations and
//! documented-but-previously-unverified guarantees (node moves across
//! parents, mid-list detach relinking, and `new_continuing_from`'s
//! cross-document non-collision promise) that a single-operation unit
//! test can't exercise.

use blueice_dom::{Document, NodeData, NodeId};

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
fn a_node_can_move_from_one_parent_to_another_via_detach_and_append() {
    let mut doc = Document::new();
    let root = doc.root();
    let old_parent = doc.create_node(elem("div"));
    let new_parent = doc.create_node(elem("section"));
    doc.append_child(root, old_parent);
    doc.append_child(root, new_parent);

    let moved = doc.create_node(text("payload"));
    doc.append_child(old_parent, moved);
    assert_eq!(doc.parent(moved), Some(old_parent));

    doc.detach(moved);
    doc.append_child(new_parent, moved);

    assert_eq!(
        doc.parent(moved),
        Some(new_parent),
        "the node must resolve under its new parent"
    );
    assert_eq!(
        doc.children(old_parent).count(),
        0,
        "the old parent must no longer see the moved node as a child"
    );
    assert_eq!(doc.children(new_parent).collect::<Vec<_>>(), vec![moved]);
    // The node's own identity survives the move -- this is the point of
    // a stable NodeId (plan §1): a client holding `moved`'s ID from
    // before the move still resolves to the same node afterward.
    assert!(doc.contains(moved));
}

#[test]
fn a_node_can_move_to_a_specific_position_under_a_new_parent() {
    let mut doc = Document::new();
    let root = doc.root();
    let old_parent = doc.create_node(elem("ul"));
    let new_parent = doc.create_node(elem("ol"));
    doc.append_child(root, old_parent);
    doc.append_child(root, new_parent);

    let anchor = doc.create_node(text("second"));
    doc.append_child(new_parent, anchor);
    let moved = doc.create_node(text("first"));
    doc.append_child(old_parent, moved);

    doc.detach(moved);
    doc.insert_before(new_parent, moved, Some(anchor));

    assert_eq!(
        doc.children(new_parent).collect::<Vec<_>>(),
        vec![moved, anchor],
        "the moved node must land immediately before the reference, under the new parent"
    );
}

#[test]
fn detaching_a_middle_child_relinks_its_neighbors_without_touching_the_parents_ends() {
    let mut doc = Document::new();
    let root = doc.root();
    let a = doc.create_node(text("a"));
    let b = doc.create_node(text("b"));
    let c = doc.create_node(text("c"));
    doc.append_child(root, a);
    doc.append_child(root, b);
    doc.append_child(root, c);

    doc.detach(b);

    assert_eq!(doc.children(root).collect::<Vec<_>>(), vec![a, c]);
    assert_eq!(doc.next_sibling(a), Some(c));
    assert_eq!(doc.prev_sibling(c), Some(a));
    assert_eq!(
        doc.last_child(root),
        Some(c),
        "the parent's last_child must be unaffected by detaching a middle child"
    );
    assert_eq!(doc.parent(b), None);
    assert_eq!(doc.next_sibling(b), None);
    assert_eq!(doc.prev_sibling(b), None);
}

#[test]
fn detaching_the_only_child_clears_both_first_and_last_child() {
    let mut doc = Document::new();
    let root = doc.root();
    let only = doc.create_node(text("only"));
    doc.append_child(root, only);

    doc.detach(only);

    assert_eq!(doc.first_child(root), None);
    assert_eq!(doc.last_child(root), None);
    assert_eq!(doc.children(root).count(), 0);
}

#[test]
fn insert_before_a_reference_with_both_neighbors_splices_in_between() {
    let mut doc = Document::new();
    let root = doc.root();
    let a = doc.create_node(text("a"));
    let c = doc.create_node(text("c"));
    let d = doc.create_node(text("d"));
    doc.append_child(root, a);
    doc.append_child(root, c);
    doc.append_child(root, d);

    let b = doc.create_node(text("b"));
    doc.insert_before(root, b, Some(c));

    assert_eq!(
        doc.children(root).collect::<Vec<_>>(),
        vec![a, b, c, d],
        "inserting before a middle reference must not disturb nodes on either side"
    );
    assert_eq!(doc.next_sibling(a), Some(b));
    assert_eq!(doc.prev_sibling(d), Some(c));
}

#[test]
#[should_panic]
fn data_panics_on_an_id_not_in_this_document() {
    let doc = Document::new();
    // `Document::contains` is documented as the safe check before a
    // panicking accessor like `data`; this confirms `data` really does
    // panic on an ID `contains` would have rejected, rather than
    // returning stale or default data silently.
    let bogus = NodeId::from_u64(999_999);
    assert!(!doc.contains(bogus));
    doc.data(bogus);
}

#[test]
fn continuing_documents_never_allocate_a_colliding_node_id() {
    // Load-bearing for plan §1: a navigation replaces the whole
    // `Document`, but a client's cached `NodeId` from before the
    // navigation must never silently resolve to an unrelated node in
    // the replacement document (`Document::new_continuing_from`'s own
    // doc comment). This was previously asserted only in prose.
    let mut first = Document::new();
    let mut first_ids = vec![first.root()];
    for _ in 0..5 {
        let id = first.create_node(text("x"));
        first.append_child(first.root(), id);
        first_ids.push(id);
    }

    let mut second = Document::new_continuing_from(first.next_node_id());
    let mut second_ids = vec![second.root()];
    for _ in 0..5 {
        let id = second.create_node(text("y"));
        second.append_child(second.root(), id);
        second_ids.push(id);
    }

    for id in &second_ids {
        assert!(
            !first_ids.contains(id),
            "a NodeId allocated by the continuing document must never equal one from the original"
        );
    }
    // Every ID from the continuing document is numerically at or past
    // the boundary the original document reported handing off.
    let boundary = first.next_node_id();
    for id in &second_ids {
        assert!(
            id.as_u64() >= boundary,
            "continuing allocation must never dip below the handed-off boundary"
        );
    }
}

#[test]
fn next_node_id_counts_every_node_ever_allocated_including_the_root() {
    let mut doc = Document::new();
    assert_eq!(
        doc.next_node_id(),
        1,
        "the root itself consumes the first id"
    );
    doc.create_node(text("a"));
    doc.create_node(text("b"));
    assert_eq!(doc.next_node_id(), 3);
}
