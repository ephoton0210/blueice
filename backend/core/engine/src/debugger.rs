// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Core-owned routing for the versioned native debugger discovery channel.
//!
//! The listener side may live on a worker thread, but this module resolves a
//! debugger target only on the core session thread against the live
//! [`crate::TabManager`]. The currently shipped implementation deliberately
//! advertises no native execution feature as available: it establishes a
//! generation-checked target/discovery boundary without fabricating a
//! breakpoint, paused frame, runtime value, or BlueJS handle.

use crate::{TabId, TabManager};
use blueice_ipc::debugger::{
    DebuggerCapabilities, DebuggerCapability, DebuggerCapabilityReport, DebuggerCapabilityState,
    DebuggerErrorCode, DebuggerPageRealm, DebuggerReply, DebuggerRequest,
    DEBUGGER_PROTOCOL_VERSION,
};
use std::io;
use std::sync::mpsc;

/// The only browser-context identity the reference core currently owns.
pub const DEFAULT_BROWSER_CONTEXT_ID: u64 = 1;

/// A bounded debugger batch prevents a busy discovery peer from starving the
/// frontend or navigation-completion processing in the owning session loop.
const MAX_DEBUGGER_REQUESTS_PER_SESSION_TICK: usize = 64;

/// Fixed discovery bounds for features that are not installed yet. They are
/// part of the advertised future contract, not permission to inspect a stack
/// or runtime value today.
const MAX_STACK_FRAMES: u32 = 64;
const MAX_SCOPE_BINDINGS: u32 = 256;
const MAX_VALUE_PREVIEW_BYTES: u32 = 4_096;

/// Sender owned by a debugger-socket worker. It forwards one decoded request
/// to the session thread and waits for that thread's target-checked reply.
#[derive(Clone)]
pub struct DebuggerRequestSender(mpsc::Sender<DebuggerRequestEnvelope>);

/// Receiver owned exclusively by the core session thread.
pub struct DebuggerRequestReceiver(mpsc::Receiver<DebuggerRequestEnvelope>);

struct DebuggerRequestEnvelope {
    request: DebuggerRequest,
    reply: mpsc::SyncSender<DebuggerReply>,
}

/// Creates the worker-to-session hand-off for debugger discovery requests.
/// The worker never borrows a tab, page, realm, VM, or BlueJS object.
pub fn debugger_request_channel() -> (DebuggerRequestSender, DebuggerRequestReceiver) {
    let (sender, receiver) = mpsc::channel();
    (
        DebuggerRequestSender(sender),
        DebuggerRequestReceiver(receiver),
    )
}

impl DebuggerRequestSender {
    /// Routes one request to the owning session. A stopped session is a
    /// transport failure rather than a synthetic debugger reply that a caller
    /// could mistake for a live target result.
    pub fn request(&self, request: DebuggerRequest) -> io::Result<DebuggerReply> {
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        self.0
            .send(DebuggerRequestEnvelope {
                request,
                reply: reply_sender,
            })
            .map_err(|_| {
                io::Error::new(io::ErrorKind::BrokenPipe, "core debugger session ended")
            })?;
        reply_receiver.recv().map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "core debugger reply unavailable")
        })
    }
}

impl DebuggerRequestReceiver {
    /// Resolves a bounded number of worker requests against the current core
    /// state. Replies are best effort: a disconnected debugger client cannot
    /// interrupt rendering or a frontend session.
    pub fn dispatch_pending(&self, tabs: &TabManager) -> usize {
        let mut dispatched = 0;
        while dispatched < MAX_DEBUGGER_REQUESTS_PER_SESSION_TICK {
            let Ok(envelope) = self.0.try_recv() else {
                break;
            };
            let reply = handle_debugger_request(tabs, envelope.request);
            let _ = envelope.reply.send(reply);
            dispatched += 1;
        }
        dispatched
    }
}

/// Handles one post-handshake debugger request on the core session thread.
/// Only capability discovery exists at v1. A `Hello` here is rejected because
/// the socket listener owns first-message negotiation before it creates a
/// request envelope.
pub fn handle_debugger_request(tabs: &TabManager, request: DebuggerRequest) -> DebuggerReply {
    match request {
        DebuggerRequest::DescribeCapabilities { realm } => describe_capabilities(tabs, realm),
        DebuggerRequest::Hello { .. } => DebuggerReply::Error {
            code: DebuggerErrorCode::ProtocolVersion,
            message: "debugger Hello is valid only as the first request".to_string(),
        },
        DebuggerRequest::Unknown => DebuggerReply::Unsupported {
            operation: "unknown debugger request".to_string(),
            reason: "this core build does not recognize the requested debugger operation"
                .to_string(),
        },
    }
}

fn describe_capabilities(tabs: &TabManager, realm: DebuggerPageRealm) -> DebuggerReply {
    if !realm.is_well_formed() || realm.browser_context_id != DEFAULT_BROWSER_CONTEXT_ID {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "invalid debugger realm target".to_string(),
        };
    }
    let Some(page) = tabs.get(TabId::from_u64(realm.tab_id)) else {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            message: "unknown debugger tab".to_string(),
        };
    };
    if page.document_generation() != realm.realm_generation {
        return DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            message: "stale debugger realm generation".to_string(),
        };
    }

    DebuggerReply::Capabilities(DebuggerCapabilities {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        realm,
        reports: planned_capability_reports(),
        max_stack_frames: MAX_STACK_FRAMES,
        max_scope_bindings: MAX_SCOPE_BINDINGS,
        max_value_preview_bytes: MAX_VALUE_PREVIEW_BYTES,
    })
}

fn planned_capability_reports() -> Vec<DebuggerCapabilityReport> {
    [
        (
            DebuggerCapability::Breakpoints,
            "native breakpoint execution is not installed",
        ),
        (
            DebuggerCapability::PauseResume,
            "native pause and resume are not installed",
        ),
        (
            DebuggerCapability::Stepping,
            "native stepping is not installed",
        ),
        (
            DebuggerCapability::Stack,
            "native stack inspection is not installed",
        ),
        (
            DebuggerCapability::Scopes,
            "native scope inspection is not installed",
        ),
        (
            DebuggerCapability::ExceptionPolicy,
            "native exception policy is not installed",
        ),
        (
            DebuggerCapability::BoundedValues,
            "native value inspection is not installed",
        ),
    ]
    .into_iter()
    .map(|(capability, detail)| DebuggerCapabilityReport {
        capability,
        state: DebuggerCapabilityState::Planned,
        detail: detail.to_string(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_tabs() -> (TabManager, DebuggerPageRealm) {
        let mut tabs = TabManager::new(320.0, 200.0);
        let tab_id = tabs.default_tab();
        tabs.get_mut(tab_id).unwrap().load_html_str(
            "<main>debugger target</main>",
            Some("https://example.test/".to_string()),
        );
        (
            tabs,
            DebuggerPageRealm {
                browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
                tab_id: tab_id.as_u64(),
                realm_generation: 1,
            },
        )
    }

    #[test]
    fn discovery_requires_the_live_tab_and_exact_document_generation() {
        let (mut tabs, realm) = loaded_tabs();
        let reply = handle_debugger_request(&tabs, DebuggerRequest::DescribeCapabilities { realm });
        let DebuggerReply::Capabilities(capabilities) = reply else {
            panic!("the live realm must have a discovery reply")
        };
        assert_eq!(capabilities.realm, realm);
        assert_eq!(capabilities.protocol_version, DEBUGGER_PROTOCOL_VERSION);
        assert!(capabilities
            .reports
            .iter()
            .all(|report| report.state == DebuggerCapabilityState::Planned));

        tabs.get_mut(TabId::from_u64(realm.tab_id))
            .unwrap()
            .load_html_str(
                "<main>replacement</main>",
                Some("https://example.test/replacement".to_string()),
            );
        assert!(matches!(
            handle_debugger_request(&tabs, DebuggerRequest::DescribeCapabilities { realm }),
            DebuggerReply::Error {
                code: DebuggerErrorCode::StaleRealm,
                ..
            }
        ));
    }

    #[test]
    fn discovery_rejects_a_malformed_or_unknown_target() {
        let (tabs, realm) = loaded_tabs();
        assert!(matches!(
            handle_debugger_request(
                &tabs,
                DebuggerRequest::DescribeCapabilities {
                    realm: DebuggerPageRealm {
                        realm_generation: 0,
                        ..realm
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
        assert!(matches!(
            handle_debugger_request(
                &tabs,
                DebuggerRequest::DescribeCapabilities {
                    realm: DebuggerPageRealm {
                        tab_id: 999,
                        ..realm
                    },
                },
            ),
            DebuggerReply::Error {
                code: DebuggerErrorCode::InvalidTarget,
                ..
            }
        ));
    }

    #[test]
    fn queued_requests_are_applied_only_by_the_session_owner() {
        let (tabs, realm) = loaded_tabs();
        let (sender, receiver) = debugger_request_channel();
        let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
        sender
            .0
            .send(DebuggerRequestEnvelope {
                request: DebuggerRequest::DescribeCapabilities { realm },
                reply: reply_sender,
            })
            .unwrap();
        assert_eq!(receiver.dispatch_pending(&tabs), 1);
        assert!(matches!(
            reply_receiver.recv().unwrap(),
            DebuggerReply::Capabilities(_)
        ));
    }
}
