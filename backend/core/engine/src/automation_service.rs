// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned dispatch for `blueice_ipc::automation`
//! (`phase-17-automation-devtools-and-ajax/PLAN.md`'s Slice 1 item 3:
//! "Add an internal automation service in `core`"). Structured exactly
//! like [`crate::script`]'s own request/reply channel: an automation
//! connection's listener thread never touches [`crate::TabManager`]
//! directly, it only enqueues an already-decoded request and blocks for
//! its reply, so `TabManager`/`Page` state is only ever mutated from
//! the one core session thread that already owns it -- no `Arc<Mutex<_>>`
//! needed, matching this module's `script.rs` precedent rather than
//! reaching for a different concurrency shape for a second protocol.
//!
//! **Minimal first slice, matching the plan's own Phase 7/8/9 pattern**:
//! capability enforcement is real (a connection can only use what its
//! own `Hello` requested), and the controller-lease exclusivity model
//! is real and shared across every connection this service dispatches
//! for -- but `Evaluate` and `ApiWorkspaceSend` are deliberate
//! `Unsupported` placeholders (real BlueJS evaluation needs the
//! `blueice_ipc::debugger` channel Slice 2 adds; a real HTTP send needs
//! the `ApiWorkspace` request-service wiring Slice 4 adds), and
//! `SubscribeNetworkEvents` acknowledges without emitting any events
//! yet (there is no network-event source to subscribe to before Slice 4).

use crate::{Page, TabId, TabManager};
use base64::Engine as _;
use blueice_dom::NodeId;
use blueice_ipc::automation::{
    AutomationError, AutomationReply, AutomationRequest, Capability, ControllerLease,
};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::mpsc;

/// Prevents a chatty automation connection from starving frontend
/// requests or navigation completions in one session-loop turn -- same
/// reasoning and same bound as `crate::script`'s own limit.
const MAX_AUTOMATION_REQUESTS_PER_SESSION_TICK: usize = 64;

/// Identifies one accepted automation connection for the lifetime of
/// this process -- never reused, the same ABA-avoidance reasoning
/// [`crate::TabId`]/`blueice_dom::NodeId` already use, since a stale
/// connection's held capabilities/lease must never appear to belong to
/// an unrelated later connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AutomationConnectionId(u64);

/// Allocates never-repeating [`AutomationConnectionId`]s -- one
/// instance shared by every automation listener in a process, the same
/// way `TabManager`'s own tab/context ID counters are owned by the one
/// `TabManager` instance rather than by each caller.
#[derive(Debug, Default)]
pub struct AutomationConnectionIdAllocator {
    next: u64,
}

impl AutomationConnectionIdAllocator {
    pub fn allocate(&mut self) -> AutomationConnectionId {
        let id = AutomationConnectionId(self.next);
        self.next += 1;
        id
    }
}

/// Sender owned by one automation-socket connection's own thread.
/// Sending a request blocks until the core session has applied it and
/// produced its reply -- the connection thread itself never touches
/// [`TabManager`]/[`AutomationServiceState`]. Cloned (via
/// [`AutomationRequestSenderFactory::for_connection`]) from one shared
/// underlying `mpsc::Sender`, so many simultaneous connections can
/// enqueue onto the same core-session-owned [`AutomationRequestReceiver`]
/// -- each request still carries its own one-shot reply channel, so
/// replies always route back to the request that made them regardless
/// of how many connections share the queue.
#[derive(Clone)]
pub struct AutomationRequestSender {
    connection: AutomationConnectionId,
    sender: mpsc::Sender<AutomationRequestEnvelope>,
}

/// Receiver owned by the core session thread. Exactly one exists per
/// process -- every accepted automation connection's
/// [`AutomationRequestSender`] feeds the same one, via
/// [`AutomationRequestSenderFactory`].
pub struct AutomationRequestReceiver(mpsc::Receiver<AutomationRequestEnvelope>);

struct AutomationRequestEnvelope {
    connection: AutomationConnectionId,
    request: AutomationRequest,
    reply: mpsc::SyncSender<AutomationReply>,
}

/// Mints one [`AutomationRequestSender`] per accepted automation
/// connection, all feeding the single [`AutomationRequestReceiver`]
/// [`automation_request_channel`] returned alongside this factory.
/// `Clone` (a thin wrapper over the underlying `mpsc::Sender`, itself
/// `Clone`) so a listener thread that accepts connections in a loop can
/// hand each newly-accepted connection its own sender without any
/// shared mutable state.
#[derive(Clone)]
pub struct AutomationRequestSenderFactory {
    sender: mpsc::Sender<AutomationRequestEnvelope>,
}

impl AutomationRequestSenderFactory {
    pub fn for_connection(&self, connection: AutomationConnectionId) -> AutomationRequestSender {
        AutomationRequestSender {
            connection,
            sender: self.sender.clone(),
        }
    }
}

/// Builds the one shared request queue for a process's automation
/// service: a [`AutomationRequestSenderFactory`] every accepted
/// connection mints its own sender from, and the single
/// [`AutomationRequestReceiver`] the core session thread polls each
/// tick via [`AutomationRequestReceiver::dispatch_pending`].
pub fn automation_request_channel() -> (AutomationRequestSenderFactory, AutomationRequestReceiver) {
    let (sender, receiver) = mpsc::channel();
    (
        AutomationRequestSenderFactory { sender },
        AutomationRequestReceiver(receiver),
    )
}

impl AutomationRequestSender {
    /// Routes one already-decoded request to the core session and
    /// waits for its reply.
    pub fn request(&self, request: AutomationRequest) -> io::Result<AutomationReply> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(AutomationRequestEnvelope {
                connection: self.connection,
                request,
                reply: reply_sender,
            })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::BrokenPipe, "core automation session ended")
            })?;
        reply_receiver.recv().map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "core automation reply unavailable",
            )
        })
    }
}

/// Per-connection and lease-ownership state the automation service
/// tracks across many requests, kept separate from [`TabManager`]
/// itself since it's protocol bookkeeping, not page/tab state -- owned
/// by the same core session thread that owns `TabManager`, so no
/// synchronization is needed here either.
#[derive(Default)]
pub struct AutomationServiceState {
    granted: HashMap<AutomationConnectionId, HashSet<Capability>>,
    lease: Option<(AutomationConnectionId, ControllerLease)>,
    next_lease_id: u64,
    /// The session's current frame-generation counter, mirrored here so
    /// [`AutomationRequest::GetAccessibilityTree`] can stamp its
    /// [`blueice_ipc::AiSnapshot`] the same way
    /// `ClientMessage::GetRepresentation` already does -- `Page` itself
    /// has no generation counter of its own (see `page.rs`'s own
    /// `snapshot` docs), only `session.rs`'s single session-wide one,
    /// so it has to be handed in from outside on every dispatch tick
    /// via [`AutomationServiceState::observe_generation`] rather than
    /// computed here.
    current_generation: u64,
}

impl AutomationServiceState {
    fn has(&self, connection: AutomationConnectionId, capability: Capability) -> bool {
        self.granted
            .get(&connection)
            .is_some_and(|set| set.contains(&capability))
    }

    fn holds_lease(&self, connection: AutomationConnectionId) -> bool {
        matches!(&self.lease, Some((holder, _)) if *holder == connection)
    }

    /// Drops every trace of a connection that's gone (closed, or its
    /// listener thread ended) -- a lease or granted-capability set must
    /// never survive its owning connection, the same "a lease must
    /// never ... block every other client indefinitely" rule the plan
    /// states for the eventual external endpoint, applied here too.
    pub fn forget_connection(&mut self, connection: AutomationConnectionId) {
        self.granted.remove(&connection);
        if matches!(&self.lease, Some((holder, _)) if *holder == connection) {
            self.lease = None;
        }
    }

    /// Records the session's current frame-generation counter --
    /// `session.rs`'s dispatch loop calls this once per tick, right
    /// before draining pending automation requests, the same way it
    /// already reads `*generation` fresh for every `GetRepresentation`
    /// reply rather than caching a stale value.
    pub fn observe_generation(&mut self, generation: u64) {
        self.current_generation = generation;
    }

    fn current_generation(&self) -> u64 {
        self.current_generation
    }
}

/// Bundles the two pieces of automation wiring `session.rs`'s poll loop
/// needs each tick -- the shared receiver every accepted connection
/// feeds, and the connection/lease bookkeeping [`handle_automation_request`]
/// mutates while draining it. A plain tuple would work too; this
/// exists so `run_session_with_script_and_automation_requests`'s own
/// signature reads as "the automation wiring", one parameter, not two
/// more positional ones to keep in order alongside the gatekeeper
/// socket and script receiver it already takes.
pub struct AutomationRequests<'a> {
    pub receiver: &'a AutomationRequestReceiver,
    pub state: &'a mut AutomationServiceState,
}

impl AutomationRequestReceiver {
    /// Applies every request currently pending at the session boundary
    /// and returns the number dispatched.
    pub fn dispatch_pending(
        &self,
        tabs: &mut TabManager,
        state: &mut AutomationServiceState,
    ) -> usize {
        let mut dispatched = 0;
        while dispatched < MAX_AUTOMATION_REQUESTS_PER_SESSION_TICK {
            let Ok(envelope) = self.0.try_recv() else {
                break;
            };
            let reply =
                handle_automation_request(tabs, state, envelope.connection, envelope.request);
            let _ = envelope.reply.send(reply);
            dispatched += 1;
        }
        dispatched
    }
}

/// Applies one decoded automation request against `tabs`/`state` for
/// the connection identified by `connection`.
pub fn handle_automation_request(
    tabs: &mut TabManager,
    state: &mut AutomationServiceState,
    connection: AutomationConnectionId,
    request: AutomationRequest,
) -> AutomationReply {
    match request {
        AutomationRequest::Hello {
            client_name: _,
            requested_capabilities,
        } => {
            // This minimal slice grants exactly what's requested --
            // real per-client restriction policy (e.g. a DevTools UI
            // never getting ApiWorkspace) is future work, the same
            // "not solved here" scope `blueice_ipc::extension`'s own
            // minimal-slice Hello handshake documents for its
            // capability-version checking.
            let granted: HashSet<Capability> = requested_capabilities.iter().copied().collect();
            state.granted.insert(connection, granted.clone());
            AutomationReply::HelloAck {
                protocol_version: blueice_ipc::automation::AUTOMATION_PROTOCOL_VERSION,
                granted_capabilities: granted.into_iter().collect(),
            }
        }
        AutomationRequest::CreateContext => {
            require(state, connection, Capability::Lifecycle, || {
                AutomationReply::ContextCreated {
                    context_id: tabs.create_context(),
                }
            })
        }
        AutomationRequest::CloseContext { context_id } => {
            require(state, connection, Capability::Lifecycle, || {
                if tabs.close_context(context_id) {
                    AutomationReply::ContextClosed { context_id }
                } else {
                    AutomationReply::Error(AutomationError::NoSuchTarget)
                }
            })
        }
        AutomationRequest::OpenTab { context_id } => {
            require(state, connection, Capability::Lifecycle, || {
                match tabs.open_tab_in_context(context_id) {
                    Some(tab_id) => AutomationReply::TabOpened {
                        tab_id: tab_id.as_u64(),
                    },
                    None => AutomationReply::Error(AutomationError::NoSuchTarget),
                }
            })
        }
        AutomationRequest::GetDom { tab_id } => require(state, connection, Capability::Inspection, || {
            match tabs.get(TabId::from_u64(tab_id)) {
                Some(page) => AutomationReply::Dom {
                    dump: page.dom_dump(),
                },
                None => AutomationReply::Error(AutomationError::NoSuchTarget),
            }
        }),
        AutomationRequest::GetAccessibilityTree { tab_id } => {
            require(state, connection, Capability::Inspection, || {
                match tabs.get(TabId::from_u64(tab_id)) {
                    Some(page) => AutomationReply::AccessibilityTree {
                        snapshot: page.snapshot(state.current_generation(), tab_id),
                    },
                    None => AutomationReply::Error(AutomationError::NoSuchTarget),
                }
            })
        }
        AutomationRequest::Screenshot { tab_id } => {
            require(state, connection, Capability::Inspection, || {
                match tabs.get(TabId::from_u64(tab_id)) {
                    Some(page) => match page.render_visible().encode_png() {
                        Ok(bytes) => AutomationReply::Screenshot {
                            png_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                        },
                        Err(error) => AutomationReply::Error(AutomationError::Internal {
                            detail: error.to_string(),
                        }),
                    },
                    None => AutomationReply::Error(AutomationError::NoSuchTarget),
                }
            })
        }
        AutomationRequest::ResolveLocator {
            tab_id,
            css_selector,
        } => require(state, connection, Capability::LocatorsAndWaiting, || {
            match tabs.get(TabId::from_u64(tab_id)) {
                Some(page) => match resolve_locator(page, &css_selector) {
                    Ok(node_ids) => AutomationReply::LocatorResolved { node_ids },
                    Err(detail) => AutomationReply::Error(AutomationError::Unsupported { detail }),
                },
                None => AutomationReply::Error(AutomationError::NoSuchTarget),
            }
        }),
        AutomationRequest::Click { tab_id, node_id } => {
            if !state.has(connection, Capability::Input) {
                return AutomationReply::Error(AutomationError::CapabilityNotGranted {
                    capability: Capability::Input,
                });
            }
            if !state.holds_lease(connection) {
                return AutomationReply::Error(AutomationError::NoControllerLease);
            }
            match tabs.get_mut(TabId::from_u64(tab_id)) {
                Some(page) => {
                    // This slice's Click is a same-page interaction
                    // only: a link's href (Page::act's Some(String)
                    // result) needs the async gatekeeper-reviewed
                    // navigation path session.rs's own dispatch loop
                    // drives, which this synchronous request/reply
                    // channel deliberately doesn't reach into yet.
                    // Clicking a non-navigating control (a button, a
                    // checkbox) is fully handled here already.
                    let _ = page.act(NodeId::from_u64(node_id), blueice_ipc::NodeAction::Click);
                    AutomationReply::ClickAck
                }
                None => AutomationReply::Error(AutomationError::NoSuchTarget),
            }
        }
        AutomationRequest::Evaluate { .. } => AutomationReply::Error(AutomationError::Unsupported {
            detail: "Evaluate does not run real BlueJS yet -- needs the blueice_ipc::debugger channel (Slice 2)".to_string(),
        }),
        AutomationRequest::SubscribeNetworkEvents { context_id } => {
            require(state, connection, Capability::NetworkAndTracing, || {
                AutomationReply::NetworkEventsSubscribed { context_id }
            })
        }
        AutomationRequest::ApiWorkspaceSend { .. } => AutomationReply::Error(AutomationError::Unsupported {
            detail: "ApiWorkspace sending needs the request-service wiring (Slice 4)".to_string(),
        }),
        AutomationRequest::AcquireControllerLease => {
            if let Some((holder, _)) = &state.lease {
                if *holder != connection {
                    return AutomationReply::Error(AutomationError::ControllerLeaseHeldByAnotherClient);
                }
            }
            let lease = ControllerLease(format!("lease-{}", state.next_lease_id));
            state.next_lease_id += 1;
            state.lease = Some((connection, lease.clone()));
            AutomationReply::ControllerLeaseGranted { lease }
        }
        AutomationRequest::ReleaseControllerLease { lease } => match &state.lease {
            Some((holder, held)) if *holder == connection && *held == lease => {
                state.lease = None;
                AutomationReply::ControllerLeaseReleased
            }
            _ => AutomationReply::Error(AutomationError::NoControllerLease),
        },
    }
}

fn require(
    state: &AutomationServiceState,
    connection: AutomationConnectionId,
    capability: Capability,
    on_granted: impl FnOnce() -> AutomationReply,
) -> AutomationReply {
    if state.has(connection, capability) {
        on_granted()
    } else {
        AutomationReply::Error(AutomationError::CapabilityNotGranted { capability })
    }
}

/// Resolves `css_selector` against `page`'s real DOM tree via
/// `blueice_css::select` -- the same "one CSS implementation, not a
/// second one bolted on by a caller" principle the cascade itself
/// already follows. Returns every matching node's ID in document order;
/// an unparseable selector is reported as `Unsupported` (a selector
/// feature outside `phase-2-mvp-scope/PLAN.md`'s MVP CSS scope, not a
/// `core` bug).
fn resolve_locator(page: &Page, css_selector: &str) -> Result<Vec<u64>, String> {
    blueice_css::select(page.document(), css_selector)
        .map(|nodes| nodes.into_iter().map(|id| id.as_u64()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrowserContextId;

    fn loaded_tabs() -> (TabManager, TabId) {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab = tabs.default_tab();
        tabs.get_mut(tab).unwrap().load_html_str(
            "<main><button id=\"save\">Save</button><p class=\"note\">hi</p></main>",
            Some("https://example.test/".to_string()),
        );
        (tabs, tab)
    }

    fn hello(
        state: &mut AutomationServiceState,
        connection: AutomationConnectionId,
        caps: &[Capability],
    ) {
        state
            .granted
            .insert(connection, caps.iter().copied().collect());
    }

    #[test]
    fn get_dom_without_hello_is_rejected_with_capability_not_granted() {
        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::GetDom {
                    tab_id: tab.as_u64()
                },
            ),
            AutomationReply::Error(AutomationError::CapabilityNotGranted {
                capability: Capability::Inspection
            })
        );
    }

    #[test]
    fn hello_grants_exactly_what_was_requested() {
        let (mut tabs, _tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        let reply = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::Hello {
                client_name: "test".to_string(),
                requested_capabilities: vec![Capability::Inspection, Capability::Input],
            },
        );
        let AutomationReply::HelloAck {
            granted_capabilities,
            ..
        } = reply
        else {
            panic!("expected HelloAck");
        };
        let granted: HashSet<_> = granted_capabilities.into_iter().collect();
        assert_eq!(
            granted,
            [Capability::Inspection, Capability::Input]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn get_dom_after_hello_returns_the_real_dom_dump() {
        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        hello(&mut state, connection, &[Capability::Inspection]);
        let reply = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::GetDom {
                tab_id: tab.as_u64(),
            },
        );
        let AutomationReply::Dom { dump } = reply else {
            panic!("expected Dom reply")
        };
        assert_eq!(dump, tabs.get(tab).unwrap().dom_dump());
        assert!(dump.contains("<button>"));
    }

    #[test]
    fn get_dom_on_an_unknown_tab_is_no_such_target() {
        let (mut tabs, _tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        hello(&mut state, connection, &[Capability::Inspection]);
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::GetDom { tab_id: 999_999 },
            ),
            AutomationReply::Error(AutomationError::NoSuchTarget)
        );
    }

    #[test]
    fn get_accessibility_tree_requires_inspection_and_stamps_the_observed_generation() {
        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);

        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::GetAccessibilityTree {
                    tab_id: tab.as_u64()
                },
            ),
            AutomationReply::Error(AutomationError::CapabilityNotGranted {
                capability: Capability::Inspection
            })
        );

        hello(&mut state, connection, &[Capability::Inspection]);
        state.observe_generation(42);
        let AutomationReply::AccessibilityTree { snapshot } = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::GetAccessibilityTree {
                tab_id: tab.as_u64(),
            },
        ) else {
            panic!("expected AccessibilityTree reply")
        };
        assert_eq!(snapshot.generation, 42);
        assert_eq!(snapshot.tab_id, tab.as_u64());
        assert!(
            snapshot
                .nodes
                .iter()
                .any(|n| n.name.as_deref() == Some("Save")),
            "the real page's Save button must appear in the accessibility tree"
        );
    }

    #[test]
    fn get_accessibility_tree_on_an_unknown_tab_is_no_such_target() {
        let (mut tabs, _tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        hello(&mut state, connection, &[Capability::Inspection]);
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::GetAccessibilityTree { tab_id: 999_999 },
            ),
            AutomationReply::Error(AutomationError::NoSuchTarget)
        );
    }

    #[test]
    fn screenshot_requires_inspection_and_returns_a_real_decodable_png() {
        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);

        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::Screenshot {
                    tab_id: tab.as_u64()
                },
            ),
            AutomationReply::Error(AutomationError::CapabilityNotGranted {
                capability: Capability::Inspection
            })
        );

        hello(&mut state, connection, &[Capability::Inspection]);
        let AutomationReply::Screenshot { png_base64 } = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::Screenshot {
                tab_id: tab.as_u64(),
            },
        ) else {
            panic!("expected Screenshot reply")
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&png_base64)
            .expect("must be valid base64");
        assert_eq!(
            &bytes[0..8],
            b"\x89PNG\r\n\x1a\n",
            "must be a real PNG file"
        );
    }

    #[test]
    fn screenshot_on_an_unknown_tab_is_no_such_target() {
        let (mut tabs, _tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        hello(&mut state, connection, &[Capability::Inspection]);
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::Screenshot { tab_id: 999_999 },
            ),
            AutomationReply::Error(AutomationError::NoSuchTarget)
        );
    }

    #[test]
    fn resolve_locator_finds_real_matching_elements_by_css_selector() {
        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        hello(&mut state, connection, &[Capability::LocatorsAndWaiting]);
        let reply = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::ResolveLocator {
                tab_id: tab.as_u64(),
                css_selector: "#save".to_string(),
            },
        );
        let AutomationReply::LocatorResolved { node_ids } = reply else {
            panic!("expected LocatorResolved")
        };
        assert_eq!(node_ids.len(), 1);

        // A selector that matches nothing resolves to an empty list,
        // not an error -- "no matches" and "couldn't understand the
        // selector" are different outcomes.
        let reply = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::ResolveLocator {
                tab_id: tab.as_u64(),
                css_selector: ".does-not-exist".to_string(),
            },
        );
        assert_eq!(reply, AutomationReply::LocatorResolved { node_ids: vec![] });
    }

    #[test]
    fn click_requires_both_the_input_capability_and_a_held_controller_lease() {
        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);

        // Neither capability nor lease yet.
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::Click {
                    tab_id: tab.as_u64(),
                    node_id: 0,
                },
            ),
            AutomationReply::Error(AutomationError::CapabilityNotGranted {
                capability: Capability::Input
            })
        );

        // Capability granted, but still no lease. Also grant
        // LocatorsAndWaiting up front so the real node id below can be
        // resolved without a second `Hello` mid-test (a connection's
        // granted set only ever grows via a fresh `Hello`, so this is
        // one realistic handshake, not two).
        hello(
            &mut state,
            connection,
            &[Capability::Input, Capability::LocatorsAndWaiting],
        );
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::Click {
                    tab_id: tab.as_u64(),
                    node_id: 0,
                },
            ),
            AutomationReply::Error(AutomationError::NoControllerLease)
        );

        // Both present: succeeds.
        let AutomationReply::ControllerLeaseGranted { .. } = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::AcquireControllerLease,
        ) else {
            panic!("expected ControllerLeaseGranted")
        };
        let AutomationReply::LocatorResolved { node_ids } = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::ResolveLocator {
                tab_id: tab.as_u64(),
                css_selector: "#save".to_string(),
            },
        ) else {
            panic!("expected LocatorResolved")
        };
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::Click {
                    tab_id: tab.as_u64(),
                    node_id: node_ids[0],
                },
            ),
            AutomationReply::ClickAck
        );
    }

    #[test]
    fn a_second_connection_cannot_acquire_the_lease_while_the_first_holds_it() {
        let (mut tabs, _tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let first = AutomationConnectionId(1);
        let second = AutomationConnectionId(2);

        assert!(matches!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                first,
                AutomationRequest::AcquireControllerLease
            ),
            AutomationReply::ControllerLeaseGranted { .. }
        ));
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                second,
                AutomationRequest::AcquireControllerLease
            ),
            AutomationReply::Error(AutomationError::ControllerLeaseHeldByAnotherClient)
        );
    }

    #[test]
    fn releasing_a_lease_lets_a_different_connection_acquire_it() {
        let (mut tabs, _tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let first = AutomationConnectionId(1);
        let second = AutomationConnectionId(2);

        let AutomationReply::ControllerLeaseGranted { lease } = handle_automation_request(
            &mut tabs,
            &mut state,
            first,
            AutomationRequest::AcquireControllerLease,
        ) else {
            panic!("expected ControllerLeaseGranted")
        };
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                first,
                AutomationRequest::ReleaseControllerLease { lease },
            ),
            AutomationReply::ControllerLeaseReleased
        );
        assert!(matches!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                second,
                AutomationRequest::AcquireControllerLease
            ),
            AutomationReply::ControllerLeaseGranted { .. }
        ));
    }

    #[test]
    fn forgetting_a_connection_releases_its_held_lease_and_capabilities() {
        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        hello(&mut state, connection, &[Capability::Inspection]);
        assert!(matches!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::AcquireControllerLease
            ),
            AutomationReply::ControllerLeaseGranted { .. }
        ));

        state.forget_connection(connection);

        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::GetDom {
                    tab_id: tab.as_u64()
                },
            ),
            AutomationReply::Error(AutomationError::CapabilityNotGranted {
                capability: Capability::Inspection
            }),
            "capabilities must not survive forget_connection"
        );
        let other = AutomationConnectionId(2);
        assert!(
            matches!(
                handle_automation_request(
                    &mut tabs,
                    &mut state,
                    other,
                    AutomationRequest::AcquireControllerLease
                ),
                AutomationReply::ControllerLeaseGranted { .. }
            ),
            "a lease must not survive forget_connection either"
        );
    }

    #[test]
    fn context_lifecycle_requires_the_lifecycle_capability_and_is_visible_via_get_dom() {
        let (mut tabs, _tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);

        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::CreateContext
            ),
            AutomationReply::Error(AutomationError::CapabilityNotGranted {
                capability: Capability::Lifecycle
            })
        );

        hello(
            &mut state,
            connection,
            &[Capability::Lifecycle, Capability::Inspection],
        );
        let AutomationReply::ContextCreated { context_id } = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::CreateContext,
        ) else {
            panic!("expected ContextCreated")
        };
        let new_tab = tabs.open_tab_in_context(context_id).unwrap();
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::GetDom {
                    tab_id: new_tab.as_u64()
                },
            ),
            AutomationReply::Dom {
                dump: String::new()
            },
            "a freshly opened, never-navigated tab has an empty DOM dump"
        );

        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::CloseContext { context_id },
            ),
            AutomationReply::ContextClosed { context_id }
        );
        assert!(tabs.get(new_tab).is_none());
    }

    #[test]
    fn open_tab_requires_lifecycle_and_places_the_tab_under_the_given_context() {
        let (mut tabs, _tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);

        let context_id = tabs.default_context();
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::OpenTab { context_id },
            ),
            AutomationReply::Error(AutomationError::CapabilityNotGranted {
                capability: Capability::Lifecycle
            })
        );

        hello(&mut state, connection, &[Capability::Lifecycle]);
        let AutomationReply::TabOpened { tab_id } = handle_automation_request(
            &mut tabs,
            &mut state,
            connection,
            AutomationRequest::OpenTab { context_id },
        ) else {
            panic!("expected TabOpened")
        };
        assert_eq!(tabs.context_of(TabId::from_u64(tab_id)), Some(context_id));
    }

    #[test]
    fn open_tab_on_an_unknown_context_is_no_such_target() {
        let (mut tabs, _tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        hello(&mut state, connection, &[Capability::Lifecycle]);

        let bogus_context = BrowserContextId(999_999);
        assert_eq!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::OpenTab {
                    context_id: bogus_context,
                },
            ),
            AutomationReply::Error(AutomationError::NoSuchTarget)
        );
    }

    #[test]
    fn evaluate_and_api_workspace_send_are_explicit_unsupported_placeholders() {
        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(1);
        assert!(matches!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::Evaluate {
                    tab_id: tab.as_u64(),
                    expression: "1+1".to_string(),
                },
            ),
            AutomationReply::Error(AutomationError::Unsupported { .. })
        ));
        assert!(matches!(
            handle_automation_request(
                &mut tabs,
                &mut state,
                connection,
                AutomationRequest::ApiWorkspaceSend {
                    method: "GET".to_string(),
                    url: "https://example.com".to_string(),
                    headers: vec![],
                },
            ),
            AutomationReply::Error(AutomationError::Unsupported { .. })
        ));
    }

    #[test]
    fn request_worker_cannot_mutate_state_until_the_session_dispatches_it() {
        use std::thread;
        use std::time::Duration;

        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        let connection = AutomationConnectionId(7);
        let (factory, receiver) = automation_request_channel();
        let sender = factory.for_connection(connection);

        let worker = thread::spawn(move || {
            sender.request(AutomationRequest::GetDom {
                tab_id: tab.as_u64(),
            })
        });

        let envelope = receiver
            .0
            .recv_timeout(Duration::from_secs(1))
            .expect("worker must enqueue a request without borrowing TabManager");
        assert_eq!(envelope.connection, connection);
        let reply =
            handle_automation_request(&mut tabs, &mut state, envelope.connection, envelope.request);
        let is_denied = matches!(
            reply,
            AutomationReply::Error(AutomationError::CapabilityNotGranted { .. })
        );
        assert!(
            is_denied,
            "the worker's connection never sent Hello, so its request must still be denied when dispatched"
        );
        let _ = envelope.reply.send(reply);
        assert!(matches!(
            worker.join().unwrap(),
            Ok(AutomationReply::Error(
                AutomationError::CapabilityNotGranted { .. }
            ))
        ));
    }

    #[test]
    fn two_connections_from_the_same_factory_enqueue_onto_the_one_shared_receiver() {
        let (factory, receiver) = automation_request_channel();
        let first = factory.for_connection(AutomationConnectionId(1));
        let second = factory.for_connection(AutomationConnectionId(2));

        let (mut tabs, tab) = loaded_tabs();
        let mut state = AutomationServiceState::default();
        hello(
            &mut state,
            AutomationConnectionId(1),
            &[Capability::Inspection],
        );
        hello(
            &mut state,
            AutomationConnectionId(2),
            &[Capability::Inspection],
        );

        let worker_one = std::thread::spawn(move || {
            first.request(AutomationRequest::GetDom {
                tab_id: tab.as_u64(),
            })
        });
        let worker_two = std::thread::spawn(move || {
            second.request(AutomationRequest::GetDom {
                tab_id: tab.as_u64(),
            })
        });

        // Both connections' requests land on the one receiver the
        // session thread owns -- dispatch whatever has arrived so far,
        // twice, since the two workers race to enqueue.
        let mut seen = HashSet::new();
        for _ in 0..2 {
            let envelope = receiver
                .0
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("both connections must enqueue onto the same receiver");
            seen.insert(envelope.connection);
            let reply = handle_automation_request(
                &mut tabs,
                &mut state,
                envelope.connection,
                envelope.request,
            );
            let _ = envelope.reply.send(reply);
        }

        assert_eq!(
            seen,
            HashSet::from([AutomationConnectionId(1), AutomationConnectionId(2)]),
            "the shared receiver must see requests from both connections, not just one"
        );
        assert!(matches!(
            worker_one.join().unwrap(),
            Ok(AutomationReply::Dom { .. })
        ));
        assert!(matches!(
            worker_two.join().unwrap(),
            Ok(AutomationReply::Dom { .. })
        ));
    }

    #[test]
    fn connection_id_allocator_never_reuses_an_id() {
        let mut allocator = AutomationConnectionIdAllocator::default();
        let a = allocator.allocate();
        let b = allocator.allocate();
        assert_ne!(a, b);
    }
}
