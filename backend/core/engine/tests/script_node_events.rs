// This Source Code Form is subject to the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_engine::{script::handle_script_request, TabManager};
use blueice_ipc::script::{ScriptDocumentTarget, ScriptReply, ScriptRequest};

fn fixture() -> (TabManager, ScriptDocumentTarget) {
    let mut tabs = TabManager::new(300.0, 200.0);
    let tab = tabs.default_tab();
    tabs.get_mut(tab).unwrap().load_html_str(
        "<div id=parent><span id=child>text</span></div>",
        Some("https://example.test/".into()),
    );
    (
        tabs,
        ScriptDocumentTarget {
            tab_id: tab.as_u64(),
            document_generation: 1,
        },
    )
}
fn lookup(tabs: &mut TabManager, target: ScriptDocumentTarget, id: &str) -> u64 {
    match handle_script_request(
        tabs,
        ScriptRequest::GetElementById {
            target,
            id: id.into(),
        },
    ) {
        ScriptReply::Node { node: Some(node) } => node,
        reply => panic!("Node lookup failed: {reply:?}"),
    }
}
fn path(tabs: &mut TabManager, target: ScriptDocumentTarget, node: u64) -> Vec<u64> {
    match handle_script_request(tabs, ScriptRequest::GetEventPath { target, node }) {
        ScriptReply::EventPath { nodes } => nodes,
        reply => panic!("Event ancestry failed: {reply:?}"),
    }
}

#[test]
fn attached_event_ancestry_uses_private_handles_through_the_exact_document() {
    let (mut tabs, target) = fixture();
    let child = lookup(&mut tabs, target, "child");
    let parent = lookup(&mut tabs, target, "parent");
    let ScriptReply::NodeCreated { node: document } =
        handle_script_request(&mut tabs, ScriptRequest::GetDocument { target })
    else {
        panic!("Document missing")
    };
    let nodes = path(&mut tabs, target, child);
    assert_eq!(&nodes[..2], &[child, parent]);
    assert_eq!(nodes.last(), Some(&document));
    assert_eq!(
        nodes.iter().collect::<std::collections::HashSet<_>>().len(),
        nodes.len()
    );
    assert_eq!(path(&mut tabs, target, document), [document]);
}

#[test]
fn detached_text_nodes_gain_live_ancestry_and_reclaimed_handles_are_denied() {
    let (mut tabs, target) = fixture();
    let parent = lookup(&mut tabs, target, "parent");
    let ScriptReply::NodeCreated { node: text } = handle_script_request(
        &mut tabs,
        ScriptRequest::CreateTextNode {
            target,
            data: "detached 中文".into(),
        },
    ) else {
        panic!("Text missing")
    };
    assert_eq!(path(&mut tabs, target, text), [text]);
    assert_eq!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::AppendChild {
                target,
                parent,
                child: text
            }
        ),
        ScriptReply::Ack
    );
    assert_eq!(&path(&mut tabs, target, text)[..2], &[text, parent]);
    assert_eq!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::SetTextContent {
                target,
                node: parent,
                value: "replacement".into()
            }
        ),
        ScriptReply::Ack
    );
    assert!(matches!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::GetEventPath { target, node: text }
        ),
        ScriptReply::Error { .. }
    ));
}

#[test]
fn foreign_tab_unknown_and_stale_document_event_paths_are_denied() {
    let (mut tabs, target) = fixture();
    let child = lookup(&mut tabs, target, "child");
    let peer = tabs.open_tab();
    tabs.get_mut(peer).unwrap().load_html_str(
        "<span id=peer>other</span>",
        Some("https://example.test/peer".into()),
    );
    let other = ScriptDocumentTarget {
        tab_id: peer.as_u64(),
        document_generation: 1,
    };
    let foreign = lookup(&mut tabs, other, "peer");
    for request in [
        ScriptRequest::GetEventPath {
            target,
            node: foreign,
        },
        ScriptRequest::GetEventPath { target, node: 0 },
        ScriptRequest::GetEventPath {
            target,
            node: u64::MAX,
        },
        ScriptRequest::GetEventPath {
            target: ScriptDocumentTarget {
                document_generation: 2,
                ..target
            },
            node: child,
        },
    ] {
        assert!(matches!(
            handle_script_request(&mut tabs, request),
            ScriptReply::Error { .. }
        ));
    }
    tabs.get_mut(tabs.default_tab()).unwrap().load_html_str(
        "<div id=child>new</div>",
        Some("https://example.test/new".into()),
    );
    assert!(matches!(
        handle_script_request(
            &mut tabs,
            ScriptRequest::GetEventPath {
                target,
                node: child
            }
        ),
        ScriptReply::Error { .. }
    ));
}
