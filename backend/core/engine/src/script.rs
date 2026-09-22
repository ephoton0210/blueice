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
use blueice_ipc::script::{ScriptReply, ScriptRequest};
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
pub mod inline_runner;
pub mod javascript;
#[cfg(unix)]
pub mod javascript_child;
pub mod page_source_authorizer;
pub use declarations::{
    discover_blue_js_page_scripts, discover_blue_ts_page_scripts, BlueJsPageScriptDeclaration,
    BlueJsPageScriptKind, BlueTsPageScriptDeclaration, BLUE_TS_CLASSIC_SCRIPT_TYPE,
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
}

fn dispatch_script_request(tabs: &mut TabManager, envelope: ScriptRequestEnvelope) {
    let reply = handle_script_request(tabs, envelope.request);
    let _ = envelope.reply.send(reply);
}

/// Applies one decoded page-script request to the addressed live tab.
///
/// The dispatcher never creates a tab, reads a URL, opens a file, grants a
/// capability, or selects a default tab for a caller. An unknown tab or node
/// produces the protocol's structured error reply, including after navigation
/// replaced the old document and invalidated its node IDs.
pub fn handle_script_request(tabs: &mut TabManager, request: ScriptRequest) -> ScriptReply {
    match request {
        ScriptRequest::Hello => ScriptReply::HelloAck,
        ScriptRequest::GetElementById { tab_id, id } => {
            with_tab(tabs, tab_id, |page| ScriptReply::Node {
                node: page.script_get_element_by_id(&id).map(|node| node.as_u64()),
            })
        }
        ScriptRequest::CreateElement { tab_id, tag_name } => with_tab(tabs, tab_id, |page| {
            page.script_create_element(tag_name).map_or_else(
                |message| ScriptReply::Error { message },
                |node| ScriptReply::NodeCreated {
                    node: node.as_u64(),
                },
            )
        }),
        ScriptRequest::CreateTextNode { tab_id, data } => {
            with_tab(tabs, tab_id, |page| ScriptReply::NodeCreated {
                node: page.script_create_text_node(data).as_u64(),
            })
        }
        ScriptRequest::AppendChild {
            tab_id,
            parent,
            child,
        } => with_tab(tabs, tab_id, |page| {
            page.script_append_child(parent, child).map_or_else(
                |message| ScriptReply::Error { message },
                |_| ScriptReply::Ack,
            )
        }),
        ScriptRequest::GetTextContent { tab_id, node } => with_tab(tabs, tab_id, |page| {
            page.script_text_content(node).map_or_else(
                |message| ScriptReply::Error { message },
                |value| ScriptReply::Text { value },
            )
        }),
        ScriptRequest::SetTextContent {
            tab_id,
            node,
            value,
        } => with_tab(tabs, tab_id, |page| {
            page.script_set_text_content(node, value).map_or_else(
                |message| ScriptReply::Error { message },
                |_| ScriptReply::Ack,
            )
        }),
    }
}

fn with_tab(
    tabs: &mut TabManager,
    raw_tab_id: u64,
    operation: impl FnOnce(&mut crate::Page) -> ScriptReply,
) -> ScriptReply {
    tabs.get_mut(TabId::from_u64(raw_tab_id))
        .map(operation)
        .unwrap_or_else(|| ScriptReply::Error {
            message: format!("unknown tab {raw_tab_id}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn hello_and_lookup_are_scoped_to_the_current_tab_document() {
        let (mut tabs, tab) = loaded_tabs();
        assert_eq!(
            handle_script_request(&mut tabs, ScriptRequest::Hello),
            ScriptReply::HelloAck
        );
        let ScriptReply::Node { node: Some(label) } = handle_script_request(
            &mut tabs,
            ScriptRequest::GetElementById {
                tab_id: tab.as_u64(),
                id: "label".to_string(),
            },
        ) else {
            panic!("the page element must resolve")
        };
        assert_eq!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    tab_id: tab.as_u64(),
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
        let ScriptReply::Node { node: Some(app) } = handle_script_request(
            &mut tabs,
            ScriptRequest::GetElementById {
                tab_id: tab.as_u64(),
                id: "app".to_string(),
            },
        ) else {
            panic!("the page element must resolve")
        };
        let ScriptReply::NodeCreated { node: child } = handle_script_request(
            &mut tabs,
            ScriptRequest::CreateElement {
                tab_id: tab.as_u64(),
                tag_name: "p".to_string(),
            },
        ) else {
            panic!("element creation must return a node")
        };
        assert_eq!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::AppendChild {
                    tab_id: tab.as_u64(),
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
                    tab_id: tab.as_u64(),
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
                    tab_id: tab.as_u64(),
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
        let ScriptReply::Node { node: Some(label) } = handle_script_request(
            &mut tabs,
            ScriptRequest::GetElementById {
                tab_id: first.as_u64(),
                id: "label".to_string(),
            },
        ) else {
            panic!("the first tab element must resolve")
        };
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    tab_id: second.as_u64(),
                    node: label,
                },
            ),
            ScriptReply::Error { .. }
        ));
        tabs.get_mut(first).unwrap().load_html_str(
            "<p>replacement</p>",
            Some("https://example.test/new".to_string()),
        );
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    tab_id: first.as_u64(),
                    node: label,
                },
            ),
            ScriptReply::Error { .. }
        ));
        assert!(matches!(
            handle_script_request(
                &mut tabs,
                ScriptRequest::GetElementById {
                    tab_id: 99_999,
                    id: "app".to_string(),
                },
            ),
            ScriptReply::Error { .. }
        ));
    }

    #[test]
    fn request_worker_cannot_mutate_a_tab_until_the_session_dispatches_it() {
        use std::thread;
        use std::time::Duration;

        let (mut tabs, tab) = loaded_tabs();
        let (sender, receiver) = script_request_channel();
        let worker = thread::spawn(move || {
            sender.request(ScriptRequest::CreateTextNode {
                tab_id: tab.as_u64(),
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
}
