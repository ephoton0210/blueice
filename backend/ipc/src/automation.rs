// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The internal wire protocol between `core` and `blueice-automation`
//! (`phase-17-automation-devtools-and-ajax/PLAN.md`'s "A single native
//! automation service" decision), Slice 1 item 1: the plan's own prose
//! names this triple `AutomationCommand`/`AutomationReply`/
//! `AutomationEvent`; implemented here as [`AutomationRequest`]/
//! [`AutomationReply`]/[`AutomationEvent`] to match this crate's own
//! established `<Protocol>Request`/`<Protocol>Reply` naming
//! ([`crate::extension::ExtensionRequest`],
//! [`crate::gatekeeper::GatekeeperRequest`]) rather than the plan
//! document's prose literally. Also: capability negotiation, an error
//! taxonomy, context/tab identities, event sequencing and controller
//! leases. Distinct from, and never spoken by, an external
//! [`crate::ClientMessage`]/[`crate::ServerMessage`] client, the same
//! way [`crate::extension`] and [`crate::gatekeeper`] are their own
//! protocols rather than new variants bolted onto the client-facing one
//! -- folding automation commands in there would couple this
//! protocol's own versioning to [`crate::PROTOCOL_VERSION`]'s
//! all-or-nothing bumps and force `frontend`/`mcp-server` to carry
//! match arms for a vocabulary they never speak.
//!
//! Like [`crate::extension`], this is long-lived and needs an explicit
//! `Hello` handshake (unlike [`crate::gatekeeper`]'s deliberately
//! handshake-less one-shot shape) -- a `blueice-automation` client
//! session spans many commands and subscribes to an ongoing event
//! stream, so both sides need to agree on a protocol version and
//! negotiated capabilities before anything else.
//!
//! **This is a minimal first slice**, the same deliberate scope the
//! plan's own "Wiring design" pattern (Phases 7/8/9) uses: the seven
//! semantic command groups the plan defines (lifecycle, inspection,
//! locators/waiting, input, script/runtime, network/tracing, API
//! workspace) are represented by [`Capability`], and each group has one
//! or two real, representative commands -- not the full surface every
//! group's own plan section eventually describes (full locator
//! predicates, the debugger protocol, SOAP, WebDriver/BiDi/CDP, and the
//! Fetch/XHR request-service refactor are separate, later slices with
//! their own wire shapes). What's real here: the protocol version,
//! `Hello` handshake and capability negotiation, the controller-lease
//! exclusivity model, the structured error taxonomy, and per-context
//! event sequencing -- the structural concerns Slice 1 item 1 actually
//! asks for.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// This protocol's own version, independent of [`crate::PROTOCOL_VERSION`]
/// (the external client protocol) and every other internal protocol's
/// version in this crate -- each evolves on its own schedule.
pub const AUTOMATION_PROTOCOL_VERSION: u32 = 1;

/// A `TabManager`-owned browsing context, per the plan's "`TabManager`
/// gains a `BrowserContextId -> TabId -> Page` ownership layer" decision.
/// A newtype over the same representation [`crate::ClientMessage`]'s
/// `tab_id` already uses on the wire, so a future `TabManager` refactor
/// can map one to the other without a serialization change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BrowserContextId(pub u64);

/// The seven semantic command groups
/// `phase-17-automation-devtools-and-ajax/PLAN.md`'s "A single native
/// automation service" section defines, used both to declare what a
/// client wants in [`AutomationRequest::Hello`] and to name the missing
/// capability in [`AutomationError::CapabilityNotGranted`]. Kept as a
/// closed enum (not a free-form string like [`crate::extension`]'s
/// per-capability versions) since this protocol has a small, fixed set
/// of groups the plan itself enumerates, not an open extension-defined
/// vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Capability {
    Lifecycle,
    Inspection,
    LocatorsAndWaiting,
    Input,
    ScriptRuntime,
    NetworkAndTracing,
    ApiWorkspace,
}

/// One message a `blueice-automation` client sends to `core`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AutomationRequest {
    /// Sent once, first, on a fresh connection -- see the module docs
    /// on why this protocol, unlike [`crate::gatekeeper`], needs one.
    /// `requested_capabilities` declares which of the seven groups this
    /// client actually wants; `core` grants a subset (see
    /// [`AutomationReply::HelloAck`]) rather than assuming every
    /// connection wants every capability.
    Hello {
        client_name: String,
        requested_capabilities: Vec<Capability>,
    },
    /// **Lifecycle.** Creates a new browser context; `core` assigns and
    /// returns its [`BrowserContextId`]. The existing default context
    /// (today's un-refactored single-`Page`-per-tab world) keeps
    /// resolving to the same identity it always has, per the plan's
    /// "retains the current `tab_id` behavior" requirement -- this
    /// command adds new contexts, it does not replace the default one.
    CreateContext,
    /// **Lifecycle.** Closes a context and every page/tab under it.
    CloseContext { context_id: BrowserContextId },
    /// **Lifecycle.** Opens a new, blank tab/page under `context_id`,
    /// returning its `tab_id` -- the automation-protocol counterpart to
    /// [`crate::ClientMessage::OpenTab`], scoped to a specific context
    /// rather than always landing in the one default context. This
    /// minimal slice opens a blank page only; requesting an initial URL
    /// (like `ClientMessage::OpenTab`'s own `url` field) needs the same
    /// gated-navigation completion path a later slice wires into this
    /// protocol, not this synchronous request/reply shape.
    OpenTab { context_id: BrowserContextId },
    /// **Inspection.** The full DOM tree dump
    /// ([`blueice_dom::dump`]'s canonical text format) for one tab --
    /// the same data [`crate::ClientMessage::GetDom`] already exposes
    /// to an external client, reachable here too since DevTools/
    /// automation clients are a distinct audience from `frontend`/MCP.
    GetDom { tab_id: u64 },
    /// **Inspection.** The same accessibility-tree-shaped schema
    /// [`crate::ClientMessage::GetRepresentation`] already exposes to
    /// an external client ([`crate::AiSnapshot`]) -- role, state,
    /// provenance-tagged name, bounds -- for one tab. Distinct from
    /// [`AutomationRequest::GetDom`]'s raw markup dump: this is the
    /// "what does an assistive technology / AI see" surface Slice 1
    /// item 4's "DOM/AX ... inspection" calls for.
    GetAccessibilityTree { tab_id: u64 },
    /// **Inspection.** A PNG screenshot of the tab's most recently
    /// rendered frame, base64-encoded on the wire (this protocol is
    /// JSON; raw bytes would need escaping anyway) -- the automation
    /// counterpart to `blueice-mcp-server`'s own `screenshot` tool,
    /// reachable here since DevTools/automation clients are a distinct
    /// audience from an MCP-driving LLM.
    Screenshot { tab_id: u64 },
    /// **Locators and waiting.** Resolves a CSS selector to the
    /// matching elements' stable [`crate::AiNode`] IDs, scoped to the
    /// document generation current when this request is served -- per
    /// the plan's "stale after navigation" rule, a caller must treat a
    /// resolved ID as invalid once [`AutomationEvent::Navigated`]
    /// reports a new generation for this tab, though enforcing that
    /// staleness at the wire level is a later slice's work, not this
    /// one's.
    ResolveLocator { tab_id: u64, css_selector: String },
    /// **Input.** Clicks the element with this node ID -- reuses
    /// [`crate::NodeAction::Click`]'s own semantics (follows a link's
    /// href if it is or is inside one) rather than inventing a second
    /// click contract.
    Click { tab_id: u64, node_id: u64 },
    /// **Script/runtime.** Placeholder for BlueJS realm evaluation:
    /// this slice defines the request/reply shape and its capability
    /// gating, not real script execution wiring (that needs the
    /// `blueice_ipc::debugger` channel and BlueJS host integration
    /// Slice 2 adds) -- see [`AutomationReply::EvaluateResult`].
    Evaluate { tab_id: u64, expression: String },
    /// **Network and tracing.** Subscribes this connection to
    /// [`AutomationEvent::NetworkRequestStarted`]/`NetworkRequestFinished`
    /// events for one context, going forward. Unsubscription and
    /// metadata-vs-body-preview access levels are later work.
    SubscribeNetworkEvents { context_id: BrowserContextId },
    /// **API workspace.** Sends one operator-authored HTTP request
    /// through the shared request path, distinct from a page's own
    /// fetches -- see the plan's `ApiWorkspace` decision. Requires
    /// [`Capability::ApiWorkspace`] *and* an acquired controller lease,
    /// same as every mutating command.
    ApiWorkspaceSend {
        method: String,
        url: String,
        headers: Vec<(String, String)>,
    },
    /// Acquires the exclusive controller lease every mutating command
    /// (`Click`, `Evaluate`, `ApiWorkspaceSend`, a future debugger
    /// pause, ...) requires, per the plan's "Mutating commands ...
    /// require an exclusive controller lease" rule. Read-only inspector
    /// sessions (`GetDom`, `ResolveLocator`) need no lease.
    AcquireControllerLease,
    /// Releases a previously acquired lease. `core` also releases it
    /// automatically if the holding connection closes -- a lease must
    /// never survive its owning session and block every other client
    /// indefinitely.
    ReleaseControllerLease { lease: ControllerLease },
}

/// An opaque, server-issued token proving a client holds the exclusive
/// controller lease. Deliberately not `Copy`/`Clone`-derived-into-misuse
/// friendly (it is `Clone` for wire transport, but callers should treat
/// possession, not the value's contents, as the capability) -- the same
/// "a typed token proves the precondition, not a runtime convention"
/// shape `phase-7-local-ai/PLAN.md`'s `GatekeeperClearance` established,
/// applied here at the protocol-message level since this token crosses
/// the wire rather than staying in-process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControllerLease(pub String);

/// `core`'s reply to one [`AutomationRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AutomationReply {
    /// Reply to [`AutomationRequest::Hello`]. `granted_capabilities` is
    /// always a subset of what was requested -- `core` never silently
    /// grants more than a client asked for -- and
    /// `protocol_version` lets a client confirm it matches
    /// [`AUTOMATION_PROTOCOL_VERSION`] before sending anything else.
    HelloAck {
        protocol_version: u32,
        granted_capabilities: Vec<Capability>,
    },
    ContextCreated {
        context_id: BrowserContextId,
    },
    ContextClosed {
        context_id: BrowserContextId,
    },
    TabOpened {
        tab_id: u64,
    },
    Dom {
        dump: String,
    },
    AccessibilityTree {
        snapshot: crate::AiSnapshot,
    },
    Screenshot {
        png_base64: String,
    },
    LocatorResolved {
        node_ids: Vec<u64>,
    },
    ClickAck,
    /// Reply to [`AutomationRequest::Evaluate`] -- this slice's
    /// placeholder shape (a plain string result), not BlueJS's eventual
    /// structured serializable-value/error contract.
    EvaluateResult {
        result: String,
    },
    NetworkEventsSubscribed {
        context_id: BrowserContextId,
    },
    ApiWorkspaceResponse {
        status: u16,
        headers: Vec<(String, String)>,
    },
    ControllerLeaseGranted {
        lease: ControllerLease,
    },
    ControllerLeaseReleased,
    /// A request failed -- always this structured taxonomy, never a
    /// bare/generic error string, mirroring
    /// [`crate::gatekeeper::GatekeeperReply::Rejected`] and
    /// [`crate::extension::ExtensionReply::CapabilityDenied`]'s own
    /// "structured, not opaque" precedent.
    Error(AutomationError),
}

/// The error taxonomy Slice 1 item 1 calls for. A closed set (not a
/// free-form message-only error) so a client can branch on the category
/// -- e.g. retry-after-navigation for `StaleHandle`, prompt-for-upgrade
/// for `Unsupported` -- without parsing prose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AutomationError {
    /// The requesting connection doesn't hold a capability this command
    /// needs -- `core` never silently no-ops a capability-gated command.
    CapabilityNotGranted { capability: Capability },
    /// A mutating command arrived without a currently-held controller
    /// lease (or with one that's since been released/superseded).
    NoControllerLease,
    /// [`AutomationRequest::AcquireControllerLease`] arrived while a
    /// *different* connection already holds the lease -- distinct from
    /// [`AutomationError::NoControllerLease`] (this caller holds none)
    /// so a client can tell "acquire failed because someone else has
    /// it, retry later" from "you forgot to acquire one at all."
    ControllerLeaseHeldByAnotherClient,
    /// A `context_id`/`tab_id`/node ID doesn't resolve to anything --
    /// distinct from `StaleHandle` (it *used to* resolve).
    NoSuchTarget,
    /// A [`AutomationRequest::ResolveLocator`] result (or any other
    /// generation-bound handle) is no longer valid because its document
    /// has since navigated or the node was removed -- the plan's "never
    /// silently retargets a similarly matching element" guarantee: a
    /// stale handle is reported, never quietly reused.
    StaleHandle,
    /// A named command/field exists in this protocol version's schema
    /// but this build doesn't implement its behavior yet -- this
    /// minimal slice's own placeholders (real `Evaluate`, `ApiWorkspaceSend`
    /// execution, ...) report this rather than pretending to succeed.
    Unsupported { detail: String },
    /// Anything else, with a human-readable detail -- an escape hatch
    /// for genuinely unanticipated failures, not a substitute for adding
    /// a real category once a failure mode is understood.
    Internal { detail: String },
}

/// One event on the ordered `AutomationEvent` stream the plan's "Event
/// ordering, resource budgets, and access control" section describes.
/// Every variant carries `context_id` and a monotonic `sequence`, per
/// context -- a client can detect a gap (dropped/coalesced events) by
/// checking for a jump in `sequence`, though the actual
/// `EventsDropped`/`BodyEvicted` eviction markers that section also
/// describes are later work, not modeled by this minimal slice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AutomationEvent {
    Navigated {
        context_id: BrowserContextId,
        tab_id: u64,
        url: String,
        generation: u64,
        sequence: u64,
    },
    ConsoleMessage {
        context_id: BrowserContextId,
        tab_id: u64,
        text: String,
        sequence: u64,
    },
    NetworkRequestStarted {
        context_id: BrowserContextId,
        tab_id: u64,
        request_id: u64,
        url: String,
        sequence: u64,
    },
    NetworkRequestFinished {
        context_id: BrowserContextId,
        tab_id: u64,
        request_id: u64,
        status: u16,
        sequence: u64,
    },
}

pub fn write_automation_request<W: Write>(w: &mut W, msg: &AutomationRequest) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_automation_request<R: Read>(r: &mut R) -> io::Result<AutomationRequest> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_automation_reply<W: Write>(w: &mut W, msg: &AutomationReply) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_automation_reply<R: Read>(r: &mut R) -> io::Result<AutomationReply> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_automation_event<W: Write>(w: &mut W, msg: &AutomationEvent) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_automation_event<R: Read>(r: &mut R) -> io::Result<AutomationEvent> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// Where `core`'s internal automation service listens, and where the
/// `blueice-automation` adapter process connects by default in
/// production -- per the plan, this is *not* the externally reachable
/// socket (`blueice-automation` "exposes secure external transports and
/// translates them to the internal protocol"; the outward-facing
/// per-user-socket-plus-capability-token endpoint the plan's "Event
/// ordering, resource budgets, and access control" section describes is
/// `blueice-automation`'s own concern, not this internal one). Mirrors
/// every other internal protocol's per-user-temp-dir convention in this
/// crate; production code is the only caller that uses this default
/// directly, tests thread an explicit path through instead.
pub fn default_automation_socket_path() -> PathBuf {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("blueice"),
        None => std::env::temp_dir().join(format!("blueice-{}", unsafe { libc_getuid() })),
    };
    dir.join("automation.sock")
}

// Duplicated from `blueice-launcher`'s (and `crate::gatekeeper`'s,
// `crate::extension`'s) identical helper rather than shared via a
// common dependency -- see those crates' own docs for why. A direct
// `extern "C"` declaration of the real POSIX `getuid()`, not a
// Linux-only `/proc/self/status` parse: that file doesn't exist on
// every Unix this workspace targets (see `backend/ipc/src/gatekeeper.rs`'s
// own fix and its regression test for the concrete failure this avoids
// from the start here -- two processes computing two different,
// unshared socket paths because a fallback used this process's own PID
// instead of a real UID).
extern "C" {
    fn getuid() -> u32;
}

unsafe fn libc_getuid() -> u32 {
    getuid()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn automation_request_round_trips_over_a_real_socket() {
        for req in [
            AutomationRequest::Hello {
                client_name: "devtools-ui".to_string(),
                requested_capabilities: vec![Capability::Inspection, Capability::Input],
            },
            AutomationRequest::CreateContext,
            AutomationRequest::CloseContext {
                context_id: BrowserContextId(1),
            },
            AutomationRequest::OpenTab {
                context_id: BrowserContextId(1),
            },
            AutomationRequest::GetDom { tab_id: 1 },
            AutomationRequest::GetAccessibilityTree { tab_id: 1 },
            AutomationRequest::Screenshot { tab_id: 1 },
            AutomationRequest::ResolveLocator {
                tab_id: 1,
                css_selector: "#save".to_string(),
            },
            AutomationRequest::Click {
                tab_id: 1,
                node_id: 42,
            },
            AutomationRequest::Evaluate {
                tab_id: 1,
                expression: "1 + 1".to_string(),
            },
            AutomationRequest::SubscribeNetworkEvents {
                context_id: BrowserContextId(0),
            },
            AutomationRequest::ApiWorkspaceSend {
                method: "GET".to_string(),
                url: "https://example.com".to_string(),
                headers: vec![("accept".to_string(), "application/json".to_string())],
            },
            AutomationRequest::AcquireControllerLease,
            AutomationRequest::ReleaseControllerLease {
                lease: ControllerLease("lease-abc".to_string()),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_automation_request(&mut a, &req).unwrap();
            assert_eq!(read_automation_request(&mut b).unwrap(), req);
        }
    }

    #[test]
    fn automation_reply_round_trips_over_a_real_socket() {
        for reply in [
            AutomationReply::HelloAck {
                protocol_version: AUTOMATION_PROTOCOL_VERSION,
                granted_capabilities: vec![Capability::Inspection],
            },
            AutomationReply::ContextCreated {
                context_id: BrowserContextId(1),
            },
            AutomationReply::ContextClosed {
                context_id: BrowserContextId(1),
            },
            AutomationReply::TabOpened { tab_id: 1 },
            AutomationReply::Dom {
                dump: "| <html>\n".to_string(),
            },
            AutomationReply::AccessibilityTree {
                snapshot: crate::AiSnapshot {
                    generation: 1,
                    tab_id: 1,
                    url: Some("https://example.test/".to_string()),
                    scroll_y: 0.0,
                    nodes: vec![],
                },
            },
            AutomationReply::Screenshot {
                png_base64: "iVBORw0KGgo=".to_string(),
            },
            AutomationReply::LocatorResolved {
                node_ids: vec![1, 2, 3],
            },
            AutomationReply::ClickAck,
            AutomationReply::EvaluateResult {
                result: "2".to_string(),
            },
            AutomationReply::NetworkEventsSubscribed {
                context_id: BrowserContextId(0),
            },
            AutomationReply::ApiWorkspaceResponse {
                status: 200,
                headers: vec![("content-type".to_string(), "text/plain".to_string())],
            },
            AutomationReply::ControllerLeaseGranted {
                lease: ControllerLease("lease-abc".to_string()),
            },
            AutomationReply::ControllerLeaseReleased,
            AutomationReply::Error(AutomationError::CapabilityNotGranted {
                capability: Capability::Input,
            }),
            AutomationReply::Error(AutomationError::NoControllerLease),
            AutomationReply::Error(AutomationError::ControllerLeaseHeldByAnotherClient),
            AutomationReply::Error(AutomationError::NoSuchTarget),
            AutomationReply::Error(AutomationError::StaleHandle),
            AutomationReply::Error(AutomationError::Unsupported {
                detail: "Evaluate does not run real BlueJS yet".to_string(),
            }),
            AutomationReply::Error(AutomationError::Internal {
                detail: "unexpected".to_string(),
            }),
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_automation_reply(&mut a, &reply).unwrap();
            assert_eq!(read_automation_reply(&mut b).unwrap(), reply);
        }
    }

    #[test]
    fn automation_event_round_trips_over_a_real_socket() {
        for event in [
            AutomationEvent::Navigated {
                context_id: BrowserContextId(0),
                tab_id: 1,
                url: "https://example.com".to_string(),
                generation: 1,
                sequence: 1,
            },
            AutomationEvent::ConsoleMessage {
                context_id: BrowserContextId(0),
                tab_id: 1,
                text: "hello".to_string(),
                sequence: 2,
            },
            AutomationEvent::NetworkRequestStarted {
                context_id: BrowserContextId(0),
                tab_id: 1,
                request_id: 7,
                url: "https://example.com/api".to_string(),
                sequence: 3,
            },
            AutomationEvent::NetworkRequestFinished {
                context_id: BrowserContextId(0),
                tab_id: 1,
                request_id: 7,
                status: 200,
                sequence: 4,
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_automation_event(&mut a, &event).unwrap();
            assert_eq!(read_automation_event(&mut b).unwrap(), event);
        }
    }

    #[test]
    fn multiple_requests_can_be_written_and_read_in_sequence_on_one_stream() {
        let mut buf = Vec::new();
        write_automation_request(&mut buf, &AutomationRequest::CreateContext).unwrap();
        write_automation_request(&mut buf, &AutomationRequest::GetDom { tab_id: 1 }).unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        assert_eq!(
            read_automation_request(&mut cursor).unwrap(),
            AutomationRequest::CreateContext
        );
        assert_eq!(
            read_automation_request(&mut cursor).unwrap(),
            AutomationRequest::GetDom { tab_id: 1 }
        );
    }

    #[test]
    fn default_automation_socket_path_is_distinct_from_the_other_well_known_socket_paths() {
        let path = default_automation_socket_path();
        assert_eq!(path.file_name().unwrap(), "automation.sock");
        assert_ne!(path.file_name().unwrap(), "core.sock");
        assert_ne!(path.file_name().unwrap(), "ai-gatekeeper.sock");
        assert_ne!(path.file_name().unwrap(), "extension-host.sock");
    }

    #[test]
    fn libc_getuid_matches_the_real_process_uid_reported_by_a_second_independent_process() {
        // See backend/ipc/src/gatekeeper.rs's identical test/fix for the
        // full explanation of why this matters for a socket-path
        // discovery mechanism specifically.
        let here = unsafe { libc_getuid() };
        let output = std::process::Command::new("id")
            .arg("-u")
            .output()
            .expect("`id -u` must run for this test to mean anything");
        let there: u32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .expect("`id -u` must print a plain number");
        assert_eq!(here, there, "libc_getuid must agree with a real independent process's own getuid(), not this process's PID");
    }

    #[test]
    fn reading_malformed_json_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        let bad_payload = b"not json";
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_automation_request(&mut cursor).is_err());
    }

    #[test]
    fn reading_a_truncated_frame_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        write_automation_reply(&mut buf, &AutomationReply::ControllerLeaseReleased).unwrap();
        buf.truncate(buf.len() - 1);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_automation_reply(&mut cursor).is_err());
    }
}
