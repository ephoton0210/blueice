// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned dispatch for the narrow `blueice_ipc::script` DOM vocabulary.
//!
//! The BlueJS host remains a separate process and is the only intended client
//! of this protocol. This module deliberately receives already-decoded IPC
//! data and a live [`crate::TabManager`], so a script operation is always
//! resolved against one current tab/document rather than a process-global DOM.

use crate::{TabId, TabManager};
use blueice_ipc::script::{
    ScriptDocumentTarget, ScriptReply, ScriptRequest, SCRIPT_MAX_NAME_BYTES, SCRIPT_MAX_TEXT_BYTES,
};
use std::io;
use std::sync::mpsc;

/// Prevents a chatty script connection from starving frontend requests or
/// navigation completions in one session-loop turn.
const MAX_SCRIPT_REQUESTS_PER_SESSION_TICK: usize = 64;

/// First live host-to-script contract inventory for direct BlueTS page
/// bindings. It contains only bindings that this core host actually installs.
pub mod contracts;
mod declarations;
pub mod direct_page;
/// Deterministic host typing artifacts derived from the core-owned binding
/// surface. The empty profile deliberately exposes no JavaScript globals;
/// `core-script-document-text-v1` is the one checked-in profile with a
/// matching direct-page runtime binding.
pub mod host_typings;
/// Immutable startup-configured HTTP(S) resource authority for the optional
/// launcher-supervised page host. It returns closed graphs only and exposes no
/// network or resolver capability to page code.
#[cfg(unix)]
pub mod http_resource_authorizer;
pub mod inline_runner;
pub mod javascript;
#[cfg(unix)]
pub mod javascript_child;
pub mod page_source_authorizer;
pub use declarations::{
    discover_blue_js_page_scripts, discover_blue_ts_page_scripts, discover_combined_page_scripts,
    BlueJsPageScriptDeclaration, BlueJsPageScriptKind, BlueTsPageScriptDeclaration,
    CombinedPageScriptDeclaration, CombinedPageScriptLanguage, BLUE_TS_CLASSIC_SCRIPT_TYPE,
    BLUE_TS_MODULE_SCRIPT_TYPE,
};

/// Sender owned by a script-socket worker. Sending a request blocks until the
/// core session has applied it to the currently live [`TabManager`] and
/// produced its reply; the worker itself never touches DOM state.
#[derive(Clone)]
pub struct ScriptRequestSender(mpsc::Sender<ScriptRequestEnvelope>);

/// Receiver owned by the core session thread. It may dispatch a bounded batch
/// between frontend reads without moving [`TabManager`] across threads.
pub struct ScriptRequestReceiver(mpsc::Receiver<ScriptRequestEnvelope>);

struct ScriptRequestEnvelope {
    request: ScriptRequest,
    reply: mpsc::SyncSender<ScriptReply>,
}

/// Creates the in-process hand-off between an IPC listener and its owning core
/// session. This is deliberately transport-neutral so tests can prove the
/// thread boundary without granting a worker direct DOM access.
pub fn script_request_channel() -> (ScriptRequestSender, ScriptRequestReceiver) {
    let (sender, receiver) = mpsc::channel();
    (ScriptRequestSender(sender), ScriptRequestReceiver(receiver))
}

impl ScriptRequestSender {
    /// Routes one already-decoded request to the core session and waits for its
    /// reply. A disconnected session is a transport failure, not a fabricated
    /// protocol reply that could be mistaken for a DOM result.
    pub fn request(&self, request: ScriptRequest) -> io::Result<ScriptReply> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.0
            .send(ScriptRequestEnvelope {
                request,
                reply: reply_sender,
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "core script session ended"))?;
        reply_receiver
            .recv()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "core script reply unavailable"))
    }
}

impl ScriptRequestReceiver {
    /// Applies every request currently pending at the session boundary and
    /// returns the number dispatched. Responses are best-effort because a
    /// disconnected script peer must never interrupt the core render session.
    pub fn dispatch_pending(&self, tabs: &mut TabManager) -> usize {
        let mut dispatched = 0;
        while dispatched < MAX_SCRIPT_REQUESTS_PER_SESSION_TICK {
            let Ok(envelope) = self.0.try_recv() else {
                break;
            };
            dispatch_script_request(tabs, envelope);
            dispatched += 1;
        }
        dispatched
    }

    /// During a synchronous child page-host wait, serve only this execution's
    /// exact document. Other live tabs and successor generations are not
    /// eligible for nested dispatch, even from the authenticated child.
    /// The caller remains the session thread and chooses the per-poll budget.
    pub fn dispatch_pending_for_document(
        &self,
        tabs: &mut TabManager,
        target: ScriptDocumentTarget,
        budget: usize,
    ) -> usize {
        let mut dispatched = 0;
        while dispatched < budget.min(MAX_SCRIPT_REQUESTS_PER_SESSION_TICK) {
            let Ok(envelope) = self.0.try_recv() else {
                break;
            };
            if script_request_target(&envelope.request) == Some(target) {
                dispatch_script_request(tabs, envelope);
            } else {
                let _ = envelope.reply.send(ScriptReply::Error {
                    message: "script DOM request is outside the executing document".to_string(),
                });
            }
            dispatched += 1;
        }
        dispatched
    }
}

fn script_request_target(request: &ScriptRequest) -> Option<ScriptDocumentTarget> {
    match request {
        ScriptRequest::Hello { .. } => None,
        ScriptRequest::GetElementById { target, .. }
        | ScriptRequest::CreateElement { target, .. }
        | ScriptRequest::CreateTextNode { target, .. }
        | ScriptRequest::AppendChild { target, .. }
        | ScriptRequest::GetTextContent { target, .. }
        | ScriptRequest::SetTextContent { target, .. } => Some(*target),
    }
}

fn dispatch_script_request(tabs: &mut TabManager, envelope: ScriptRequestEnvelope) {
    let reply = handle_script_request(tabs, envelope.request);
    let _ = envelope.reply.send(reply);
}

/// Applies one decoded page-script request to the addressed live tab.
///
/// The dispatcher never creates a tab, reads a URL, opens a file, grants a
/// capability, or selects a default tab for a caller. An unknown tab, stale
/// document generation, or invalid node produces the protocol's structured
/// error reply before an operation can touch a successor document.
pub fn handle_script_request(tabs: &mut TabManager, request: ScriptRequest) -> ScriptReply {
    match request {
        ScriptRequest::Hello { .. } => script_error("script Hello is transport-only"),
        ScriptRequest::GetElementById { target, id } => {
            if id.len() > SCRIPT_MAX_NAME_BYTES {
                return script_error("script DOM name exceeds its fixed byte limit");
            }
            with_document(tabs, target, |page| ScriptReply::Node {
                node: page.script_get_element_by_id(&id).map(|node| node.as_u64()),
            })
        }
        ScriptRequest::CreateElement { target, tag_name } => {
            if tag_name.len() > SCRIPT_MAX_NAME_BYTES {
                return script_error("script DOM name exceeds its fixed byte limit");
            }
            with_document(tabs, target, |page| {
                page.script_create_element(tag_name).map_or_else(
                    |message| ScriptReply::Error { message },
                    |node| ScriptReply::NodeCreated {
                        node: node.as_u64(),
                    },
                )
            })
        }
        ScriptRequest::CreateTextNode { target, data } => {
            if data.len() > SCRIPT_MAX_TEXT_BYTES {
                return script_error("script DOM text exceeds its fixed byte limit");
            }
            with_document(tabs, target, |page| ScriptReply::NodeCreated {
                node: page.script_create_text_node(data).as_u64(),
            })
        }
        ScriptRequest::AppendChild {
            target,
            parent,
            child,
        } => with_document(tabs, target, |page| {
            page.script_append_child(parent, child).map_or_else(
                |message| ScriptReply::Error { message },
                |_| ScriptReply::Ack,
            )
        }),
        ScriptRequest::GetTextContent { target, node } => with_document(tabs, target, |page| {
            page.script_text_content(node).map_or_else(
                |message| ScriptReply::Error { message },
                |value| {
                    if value.len() > SCRIPT_MAX_TEXT_BYTES {
                        script_error("script DOM text exceeds its fixed byte limit")
                    } else {
                        ScriptReply::Text { value }
                    }
                },
            )
        }),
        ScriptRequest::SetTextContent {
            target,
            node,
            value,
        } => {
            if value.len() > SCRIPT_MAX_TEXT_BYTES {
                return script_error("script DOM text exceeds its fixed byte limit");
            }
            with_document(tabs, target, |page| {
                page.script_set_text_content(node, value).map_or_else(
                    |message| ScriptReply::Error { message },
                    |_| ScriptReply::Ack,
                )
            })
        }
    }
}

fn script_error(message: &'static str) -> ScriptReply {
    ScriptReply::Error {
        message: message.to_string(),
    }
}

fn with_document(
    tabs: &mut TabManager,
    target: ScriptDocumentTarget,
    operation: impl FnOnce(&mut crate::Page) -> ScriptReply,
) -> ScriptReply {
    match tabs.get_mut(TabId::from_u64(target.tab_id)) {
        Some(page) if page.document_generation() == target.document_generation => operation(page),
        _ => script_error("script document is stale or unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::script::SCRIPT_PROTOCOL_VERSION;

    fn target(tab: TabId, document_generation: u64) -> ScriptDocumentTarget {
        ScriptDocumentTarget {
            tab_id: tab.as_u64(),
            document_generation,
        }
    }

    fn loaded_tabs() -> (TabManager, TabId) {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<main id=\"app\"><span id=\"label\">old</span></main>",
            Some("https://example.test/".to_string()),
        );
        (tabs, tab)
    }

    #[test]
    fn hello_is_transport_only_and_lookup_is_scoped_to_the_current_document() {
        let (mut tabs, tab) = loaded_tabs();
        let generation = tabs.get(tab).unwrap().document_generation();
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::Hello {
                    protocol_version: SCRIPT_PROTOCOL_VERSION,
                    session_token: "a".repeat(64),
                },
            ),
            ScriptReply::Error { .. }
        ));
        let ScriptReply::Node { node: Some(label) } = handle_script_request(
            &mut tabs,
            ScriptRequest::GetElementById {
                target: target(tab, generation),
                id: "label".to_string(),
            },
        ) else {
            panic!("the page element must resolve")
        };
        assert_eq!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    target: target(tab, generation),
                    node: label,
                },
            ),
            ScriptReply::Text {
                value: "old".to_string()
            }
        );
    }

    #[test]
    fn creation_append_and_text_replacement_mutate_only_the_addressed_tab() {
        let (mut tabs, tab) = loaded_tabs();
        let generation = tabs.get(tab).unwrap().document_generation();
        let ScriptReply::Node { node: Some(app) } = handle_script_request(
            &mut tabs,
            ScriptRequest::GetElementById {
                target: target(tab, generation),
                id: "app".to_string(),
            },
        ) else {
            panic!("the page element must resolve")
        };
        let ScriptReply::NodeCreated { node: child } = handle_script_request(
            &mut tabs,
            ScriptRequest::CreateElement {
                target: target(tab, generation),
                tag_name: "p".to_string(),
            },
        ) else {
            panic!("element creation must return a node")
        };
        assert_eq!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::AppendChild {
                    target: target(tab, generation),
                    parent: app,
                    child,
                },
            ),
            ScriptReply::Ack
        );
        assert_eq!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::SetTextContent {
                    target: target(tab, generation),
                    node: child,
                    value: "new".to_string(),
                },
            ),
            ScriptReply::Ack
        );
        assert_eq!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    target: target(tab, generation),
                    node: app,
                },
            ),
            ScriptReply::Text {
                value: "oldnew".to_string()
            }
        );
        assert!(tabs.get(tab).unwrap().dom_dump().contains("\"new\""));
    }

    #[test]
    fn stale_or_cross_tab_handles_fail_without_mutation() {
        let (mut tabs, first) = loaded_tabs();
        let second = tabs.open_tab();
        let old_generation = tabs.get(first).unwrap().document_generation();
        let second_generation = tabs.get(second).unwrap().document_generation();
        let ScriptReply::Node { node: Some(label) } = handle_script_request(
            &mut tabs,
            ScriptRequest::GetElementById {
                target: target(first, old_generation),
                id: "label".to_string(),
            },
        ) else {
            panic!("the first tab element must resolve")
        };
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    target: target(second, second_generation),
                    node: label,
                },
            ),
            ScriptReply::Error { .. }
        ));
        tabs.get_mut(first).unwrap().load_html_str(
            "<p id=\"fresh\">replacement</p>",
            Some("https://example.test/new".to_string()),
        );
        let replacement_before = tabs.get(first).unwrap().dom_dump();
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    target: target(first, old_generation),
                    node: label,
                },
            ),
            ScriptReply::Error { .. }
        ));
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::CreateTextNode {
                    target: target(first, old_generation),
                    data: "must not enter successor".to_string(),
                },
            ),
            ScriptReply::Error { .. }
        ));
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::SetTextContent {
                    target: target(first, old_generation),
                    node: label,
                    value: "must not replace successor".to_string(),
                },
            ),
            ScriptReply::Error { .. }
        ));
        assert_eq!(tabs.get(first).unwrap().dom_dump(), replacement_before);
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetElementById {
                    target: target(first, old_generation + 1),
                    id: "fresh".to_string(),
                },
            ),
            ScriptReply::Node { node: Some(_) }
        ));
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetElementById {
                    target: ScriptDocumentTarget {
                        tab_id: 99_999,
                        document_generation: 1,
                    },
                    id: "app".to_string(),
                },
            ),
            ScriptReply::Error { .. }
        ));
    }

    #[test]
    fn oversized_dom_inputs_fail_before_mutating_the_current_document() {
        let (mut tabs, tab) = loaded_tabs();
        let before = tabs.get(tab).unwrap().dom_dump();
        let ScriptReply::Node { node: Some(label) } = handle_script_request(
            &mut tabs,
            ScriptRequest::GetElementById {
                target: target(tab, 1),
                id: "label".to_string(),
            },
        ) else {
            panic!("the page label must resolve")
        };
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::CreateTextNode {
                    target: target(tab, 1),
                    data: "x".repeat(SCRIPT_MAX_TEXT_BYTES + 1),
                },
            ),
            ScriptReply::Error { .. }
        ));
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetElementById {
                    target: target(tab, 1),
                    id: "x".repeat(SCRIPT_MAX_NAME_BYTES + 1),
                },
            ),
            ScriptReply::Error { .. }
        ));
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::SetTextContent {
                    target: target(tab, 1),
                    node: label,
                    value: "x".repeat(SCRIPT_MAX_TEXT_BYTES + 1),
                },
            ),
            ScriptReply::Error { .. }
        ));
        assert_eq!(tabs.get(tab).unwrap().dom_dump(), before);

        let oversized = tabs
            .get_mut(tab)
            .unwrap()
            .script_create_text_node("x".repeat(SCRIPT_MAX_TEXT_BYTES + 1));
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    target: target(tab, 1),
                    node: oversized.as_u64(),
                },
            ),
            ScriptReply::Error { .. }
        ));
    }

    #[test]
    fn stale_document_generation_cannot_create_a_node_in_its_successor() {
        let (mut tabs, tab) = loaded_tabs();
        let old_generation = tabs.get(tab).unwrap().document_generation();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<p>replacement</p>",
            Some("https://example.test/new".to_string()),
        );
        let before = tabs.get(tab).unwrap().dom_dump();
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::CreateTextNode {
                    target: target(tab, old_generation),
                    data: "old document".to_string(),
                },
            ),
            ScriptReply::Error { .. }
        ));
        assert_eq!(tabs.get(tab).unwrap().dom_dump(), before);
    }

    #[test]
    fn request_worker_cannot_mutate_a_tab_until_the_session_dispatches_it() {
        use std::thread;
        use std::time::Duration;

        let (mut tabs, tab) = loaded_tabs();
        let (sender, receiver) = script_request_channel();
        let worker = thread::spawn(move || {
            sender.request(ScriptRequest::CreateTextNode {
                target: target(tab, 1),
                data: "from-worker".to_string(),
            })
        });

        let envelope = receiver
            .0
            .recv_timeout(Duration::from_secs(1))
            .expect("worker must enqueue a request without borrowing the tab manager");
        assert!(
            !tabs.get(tab).unwrap().dom_dump().contains("from-worker"),
            "queueing itself must not mutate core-owned state"
        );
        dispatch_script_request(&mut tabs, envelope);
        assert!(matches!(
            worker.join().unwrap(),
            Ok(ScriptReply::NodeCreated { .. })
        ));
    }

    #[test]
    fn nested_dispatch_serves_only_the_exact_executing_document_with_a_batch_bound() {
        let (mut tabs, first) = loaded_tabs();
        let second = tabs.open_tab();
        let generation = tabs.get(first).unwrap().document_generation();
        let ScriptReply::Node { node: Some(label) } = handle_script_request(
            &mut tabs,
            ScriptRequest::GetElementById {
                target: target(first, generation),
                id: "label".to_string(),
            },
        ) else {
            panic!("the first document must have a label");
        };
        let first_before = tabs.get(first).unwrap().dom_dump();
        let second_before = tabs.get(second).unwrap().dom_dump();
        let (sender, receiver) = script_request_channel();
        let mut replies = Vec::new();
        for (request_target, value) in [
            (target(second, 0), "cross-tab"),
            (target(first, generation - 1), "stale"),
            (target(first, generation), "allowed"),
        ] {
            let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
            sender
                .0
                .send(ScriptRequestEnvelope {
                    request: ScriptRequest::SetTextContent {
                        target: request_target,
                        node: label,
                        value: value.to_string(),
                    },
                    reply: reply_sender,
                })
                .unwrap();
            replies.push(reply_receiver);
        }
        assert_eq!(
            receiver.dispatch_pending_for_document(&mut tabs, target(first, generation), 2),
            2
        );
        assert!(matches!(
            replies[0].recv().unwrap(),
            ScriptReply::Error { .. }
        ));
        assert!(matches!(
            replies[1].recv().unwrap(),
            ScriptReply::Error { .. }
        ));
        assert_eq!(tabs.get(first).unwrap().dom_dump(), first_before);
        assert_eq!(tabs.get(second).unwrap().dom_dump(), second_before);
        assert_eq!(
            receiver.dispatch_pending_for_document(&mut tabs, target(first, generation), 2),
            1
        );
        assert_eq!(replies[2].recv().unwrap(), ScriptReply::Ack);
        assert!(tabs.get(first).unwrap().dom_dump().contains("allowed"));
        assert_eq!(tabs.get(second).unwrap().dom_dump(), second_before);
    }
}
