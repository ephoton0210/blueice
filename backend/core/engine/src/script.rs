// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core's long-lived BlueJS DOM-request dispatcher.
//!
//! `bluejs` never receives a `Page` or `Document`; it exchanges numeric node
//! handles on this protocol and core validates every tab/node lookup here.
//! The binary-level supervisor owns accepting/spawning connections, while this
//! pure `Read + Write` loop is independently testable over a Unix socket pair.

use crate::{ScriptClassListOperation, TabId, TabManager};
use blueice_dom::NodeId;
use blueice_ipc::script::{
    ScriptClassListOperation as IpcClassListOperation, ScriptCommand, ScriptReply, ScriptRequest,
    ScriptTurnOutcome, read_script_request, write_script_command, write_script_reply,
};
use std::io::{self, Read, Write};

/// Serves a connected BlueJS host until it disconnects. A non-`Hello` first
/// message is rejected by ending the connection, matching `session` and the
/// extension-host handshake discipline.
pub fn handle_script_connection<S: Read + Write>(
    tabs: &mut TabManager,
    stream: &mut S,
) -> io::Result<()> {
    match read_script_request(stream) {
        Ok(ScriptRequest::Hello) => write_script_reply(stream, &ScriptReply::HelloAck)?,
        Ok(_) | Err(_) => return Ok(()),
    }
    loop {
        let request = match read_script_request(stream) {
            Ok(request) => request,
            Err(_) => return Ok(()),
        };
        let reply = dispatch(tabs, request);
        write_script_reply(stream, &reply)?;
    }
}

/// Core's synchronous side of one parser-script turn on the always-resident
/// BlueJS connection. While a turn is running it keeps serving DOM requests;
/// only after BlueJS sends `Complete` may the caller render the page. This is
/// the concrete bridge between the event-loop design's "scheduled script
/// before paint" rule and the process-isolated DOM owner.
pub struct ScriptSession<S> {
    stream: S,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptEventOutcome {
    pub default_prevented: bool,
    pub ran_event: bool,
}

/// The narrow seam the render/session loop uses to run a freshly loaded
/// document before its first frame. Keeping it independent of the concrete
/// socket type preserves `session`'s existing in-process testability.
pub trait ScriptScheduler {
    fn run_document_scripts(&mut self, tabs: &mut TabManager, tab_id: TabId) -> Result<(), String>;

    fn dispatch_event(
        &mut self,
        _: &mut TabManager,
        _: TabId,
        _: NodeId,
        _: &str,
    ) -> Result<ScriptEventOutcome, String> {
        Ok(ScriptEventOutcome::default())
    }

    fn run_due_timers(&mut self, _: &mut TabManager, _: TabId) -> Result<bool, String> {
        Ok(false)
    }
}

impl<S: Read + Write> ScriptSession<S> {
    /// Accepts the script host's mandatory first `Hello` once, leaving the
    /// stream at the command/request boundary for future render passes.
    pub fn accept(mut stream: S) -> io::Result<Self> {
        match read_script_request(&mut stream) {
            Ok(ScriptRequest::Hello) => {
                write_script_reply(&mut stream, &ScriptReply::HelloAck)?;
                Ok(Self { stream })
            }
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "BlueJS did not send Hello first",
            )),
            Err(error) => Err(error),
        }
    }

    /// Resets a tab realm then runs all inline classic scripts in document
    /// order. A runtime error abandons the remaining scripts for this page,
    /// while the core page keeps its last successfully-applied DOM state.
    pub fn run_document_scripts(&mut self, tabs: &mut TabManager, tab_id: TabId) -> io::Result<()> {
        self.run_turn(
            tabs,
            tab_id,
            ScriptCommand::ResetRealm {
                tab_id: tab_id.as_u64(),
            },
        )?;
        let sources = tabs
            .get(tab_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "script tab no longer exists"))?
            .inline_script_sources();
        for source in sources {
            self.run_turn(
                tabs,
                tab_id,
                ScriptCommand::Execute {
                    tab_id: tab_id.as_u64(),
                    source,
                },
            )?;
        }
        Ok(())
    }

    /// Runs one command while handling arbitrary many BlueJS DOM operations.
    pub fn run_turn(
        &mut self,
        tabs: &mut TabManager,
        expected_tab: TabId,
        command: ScriptCommand,
    ) -> io::Result<ScriptTurnOutcome> {
        write_script_command(&mut self.stream, &command)?;
        loop {
            let request = read_script_request(&mut self.stream)?;
            match request {
                ScriptRequest::Complete {
                    tab_id,
                    error,
                    outcome,
                } if tab_id == expected_tab.as_u64() => {
                    write_script_reply(&mut self.stream, &ScriptReply::Ack)?;
                    return error.map_or(Ok(outcome), |message| Err(io::Error::other(message)));
                }
                ScriptRequest::Complete { tab_id, .. } => {
                    write_script_reply(
                        &mut self.stream,
                        &ScriptReply::Error {
                            message: format!(
                                "BlueJS completed tab {tab_id} while core awaited tab {}",
                                expected_tab.as_u64()
                            ),
                        },
                    )?;
                }
                request => {
                    let reply = dispatch(tabs, request);
                    write_script_reply(&mut self.stream, &reply)?;
                }
            }
        }
    }

    pub fn into_inner(self) -> S {
        self.stream
    }

    /// Requests orderly daemon termination after core's frontend session has
    /// ended. `Shutdown` deliberately has no completion barrier: BlueJS exits
    /// after seeing it and the socket closes naturally.
    pub fn shutdown(&mut self) -> io::Result<()> {
        write_script_command(&mut self.stream, &ScriptCommand::Shutdown)
    }
}

impl<S: Read + Write> ScriptScheduler for ScriptSession<S> {
    fn run_document_scripts(&mut self, tabs: &mut TabManager, tab_id: TabId) -> Result<(), String> {
        Self::run_document_scripts(self, tabs, tab_id).map_err(|error| error.to_string())
    }

    fn dispatch_event(
        &mut self,
        tabs: &mut TabManager,
        tab_id: TabId,
        node: NodeId,
        event_type: &str,
    ) -> Result<ScriptEventOutcome, String> {
        self.run_turn(
            tabs,
            tab_id,
            ScriptCommand::DispatchEvent {
                tab_id: tab_id.as_u64(),
                node: node.as_u64(),
                event_type: event_type.to_string(),
            },
        )
        .map(|outcome| ScriptEventOutcome {
            default_prevented: outcome.default_prevented,
            ran_event: outcome.ran_event,
        })
        .map_err(|error| error.to_string())
    }

    fn run_due_timers(&mut self, tabs: &mut TabManager, tab_id: TabId) -> Result<bool, String> {
        self.run_turn(
            tabs,
            tab_id,
            ScriptCommand::RunDueTimers {
                tab_id: tab_id.as_u64(),
            },
        )
        .map(|outcome| outcome.ran_timers)
        .map_err(|error| error.to_string())
    }
}

/// One request's core-side semantics, exposed for tests and for the core event
/// loop to call when it multiplexes its always-resident script connection.
pub fn dispatch(tabs: &mut TabManager, request: ScriptRequest) -> ScriptReply {
    match request {
        ScriptRequest::Hello => ScriptReply::HelloAck,
        ScriptRequest::Complete { .. } => ScriptReply::Ack,
        ScriptRequest::GetElementById { tab_id, id } => with_page(tabs, tab_id, |page| {
            Ok(ScriptReply::Node {
                node: page.script_get_element_by_id(&id).map(|node| node.as_u64()),
            })
        }),
        ScriptRequest::QuerySelector { tab_id, selector } => with_page(tabs, tab_id, |page| {
            Ok(ScriptReply::Node {
                node: page
                    .script_query_selector(&selector)?
                    .map(|node| node.as_u64()),
            })
        }),
        ScriptRequest::QuerySelectorAll { tab_id, selector } => with_page(tabs, tab_id, |page| {
            Ok(ScriptReply::Nodes {
                nodes: page
                    .script_query_selector_all(&selector)?
                    .into_iter()
                    .map(|node| node.as_u64())
                    .collect(),
            })
        }),
        ScriptRequest::CreateElement { tab_id, tag_name } => with_page(tabs, tab_id, |page| {
            Ok(ScriptReply::NodeCreated {
                node: page.script_create_element(tag_name).as_u64(),
            })
        }),
        ScriptRequest::CreateTextNode { tab_id, data } => with_page(tabs, tab_id, |page| {
            Ok(ScriptReply::NodeCreated {
                node: page.script_create_text_node(data).as_u64(),
            })
        }),
        ScriptRequest::AppendChild {
            tab_id,
            parent,
            child,
        } => with_page(tabs, tab_id, |page| {
            page.script_append_child(NodeId::from_u64(parent), NodeId::from_u64(child))?;
            Ok(ScriptReply::Ack)
        }),
        ScriptRequest::InsertBefore {
            tab_id,
            parent,
            child,
            reference,
        } => with_page(tabs, tab_id, |page| {
            page.script_insert_before(
                NodeId::from_u64(parent),
                NodeId::from_u64(child),
                reference.map(NodeId::from_u64),
            )?;
            Ok(ScriptReply::Ack)
        }),
        ScriptRequest::RemoveChild {
            tab_id,
            parent,
            child,
        } => with_page(tabs, tab_id, |page| {
            page.script_remove_child(NodeId::from_u64(parent), NodeId::from_u64(child))?;
            Ok(ScriptReply::Ack)
        }),
        ScriptRequest::Remove { tab_id, node } => with_page(tabs, tab_id, |page| {
            page.script_remove(NodeId::from_u64(node))?;
            Ok(ScriptReply::Ack)
        }),
        ScriptRequest::GetTextContent { tab_id, node } => with_page(tabs, tab_id, |page| {
            Ok(ScriptReply::Text {
                value: page.script_text_content(NodeId::from_u64(node))?,
            })
        }),
        ScriptRequest::SetTextContent {
            tab_id,
            node,
            value,
        } => with_page(tabs, tab_id, |page| {
            page.script_set_text_content(NodeId::from_u64(node), value)?;
            Ok(ScriptReply::Ack)
        }),
        ScriptRequest::GetAttribute { tab_id, node, name } => with_page(tabs, tab_id, |page| {
            Ok(ScriptReply::Attribute {
                value: page.script_get_attribute(NodeId::from_u64(node), &name)?,
            })
        }),
        ScriptRequest::SetAttribute {
            tab_id,
            node,
            name,
            value,
        } => with_page(tabs, tab_id, |page| {
            page.script_set_attribute(NodeId::from_u64(node), name, value)?;
            Ok(ScriptReply::Ack)
        }),
        ScriptRequest::RemoveAttribute { tab_id, node, name } => with_page(tabs, tab_id, |page| {
            page.script_remove_attribute(NodeId::from_u64(node), &name)?;
            Ok(ScriptReply::Ack)
        }),
        ScriptRequest::GetInnerHtml { tab_id, node } => with_page(tabs, tab_id, |page| {
            Ok(ScriptReply::Text {
                value: page.script_inner_html(NodeId::from_u64(node))?,
            })
        }),
        ScriptRequest::GetStyleProperty {
            tab_id,
            node,
            property,
        } => with_page(tabs, tab_id, |page| {
            Ok(ScriptReply::Text {
                value: page.script_get_style_property(NodeId::from_u64(node), &property)?,
            })
        }),
        ScriptRequest::SetStyleProperty {
            tab_id,
            node,
            property,
            value,
        } => with_page(tabs, tab_id, |page| {
            page.script_set_style_property(NodeId::from_u64(node), property, value)?;
            Ok(ScriptReply::Ack)
        }),
        ScriptRequest::ClassList {
            tab_id,
            node,
            operation,
            class_name,
        } => with_page(tabs, tab_id, |page| {
            let operation = match operation {
                IpcClassListOperation::Add => ScriptClassListOperation::Add,
                IpcClassListOperation::Remove => ScriptClassListOperation::Remove,
                IpcClassListOperation::Toggle => ScriptClassListOperation::Toggle,
                IpcClassListOperation::Contains => ScriptClassListOperation::Contains,
            };
            Ok(ScriptReply::Bool {
                value: page.script_class_list(NodeId::from_u64(node), operation, class_name)?,
            })
        }),
    }
}

fn with_page(
    tabs: &mut TabManager,
    tab_id: u64,
    f: impl FnOnce(&mut crate::Page) -> Result<ScriptReply, String>,
) -> ScriptReply {
    let Some(page) = tabs.get_mut(TabId::from_u64(tab_id)) else {
        return ScriptReply::Error {
            message: format!("unknown script tab {tab_id}"),
        };
    };
    f(page).unwrap_or_else(|message| ScriptReply::Error { message })
}

#[cfg(test)]
mod tests {
    use super::*;
    use blueice_ipc::script::{read_script_reply, write_script_request};
    use std::os::unix::net::UnixStream;
    use std::thread;

    #[test]
    fn script_host_handshake_and_dom_mutations_use_core_as_the_only_dom_owner() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            let mut tabs = TabManager::new(320.0, 200.0);
            let tab = tabs.default_tab();
            tabs.get_mut(tab)
                .unwrap()
                .load_html_str("<div id='target'>old</div>", None);
            handle_script_connection(&mut tabs, &mut server)
        });

        write_script_request(&mut client, &ScriptRequest::Hello).unwrap();
        assert_eq!(
            read_script_reply(&mut client).unwrap(),
            ScriptReply::HelloAck
        );
        write_script_request(
            &mut client,
            &ScriptRequest::GetElementById {
                tab_id: 1,
                id: "target".to_string(),
            },
        )
        .unwrap();
        let target = match read_script_reply(&mut client).unwrap() {
            ScriptReply::Node { node: Some(node) } => node,
            reply => panic!("unexpected reply: {reply:?}"),
        };
        write_script_request(
            &mut client,
            &ScriptRequest::SetTextContent {
                tab_id: 1,
                node: target,
                value: "new".to_string(),
            },
        )
        .unwrap();
        assert_eq!(read_script_reply(&mut client).unwrap(), ScriptReply::Ack);
        write_script_request(
            &mut client,
            &ScriptRequest::GetTextContent {
                tab_id: 1,
                node: target,
            },
        )
        .unwrap();
        assert_eq!(
            read_script_reply(&mut client).unwrap(),
            ScriptReply::Text {
                value: "new".to_string()
            }
        );

        drop(client);
        handle.join().unwrap().unwrap();
    }

    #[test]
    fn invalid_tab_or_node_is_a_structured_reply_not_a_core_panic() {
        let mut tabs = TabManager::new(100.0, 100.0);
        assert!(matches!(
            dispatch(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    tab_id: 99,
                    node: 1
                }
            ),
            ScriptReply::Error { .. }
        ));
        assert!(matches!(
            dispatch(
                &mut tabs,
                ScriptRequest::GetTextContent {
                    tab_id: 1,
                    node: 99
                }
            ),
            ScriptReply::Error { .. }
        ));
    }
}
