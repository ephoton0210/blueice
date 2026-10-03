// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_dom::{Document, NodeData};
#[test]
fn cloned_document_keeps_ids_and_tree_but_mutations_and_allocator_are_independent() {
    let mut live = Document::new_continuing_from(42);
    let text = live.create_node(NodeData::Text {
        data: "before".into(),
    });
    live.append_child(live.root(), text);
    let mut frozen = live.clone();
    *live.data_mut(text) = NodeData::Text {
        data: "after".into(),
    };
    assert_eq!(
        frozen.data(text),
        &NodeData::Text {
            data: "before".into()
        }
    );
    let next = live.create_node(NodeData::Text { data: "new".into() });
    assert!(!frozen.contains(next));
    assert_eq!(
        frozen.children(frozen.root()).collect::<Vec<_>>(),
        vec![text]
    );
    let independent = frozen.create_node(NodeData::Text {
        data: "snapshot only".into(),
    });
    assert_eq!(next, independent);
    assert_ne!(live.data(next), frozen.data(independent));
}
