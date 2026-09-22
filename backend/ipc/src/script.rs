// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The internal wire protocol between `core` and an out-of-process
//! `bluejs` script host, per `phase-13-bluejs-engine/PLAN.md`'s
//! "Wiring design (resolved 2026-09-08)": BlueJS is isolated the same
//! way `extension`/`ai-gatekeeper` are (a script crash/hang must never
//! be able to take `core` down), so every DOM operation script performs
//! crosses this wire rather than touching `core`'s state in-process.
//!
//! Lives inside `blueice-ipc` for the same reason [`crate::extension`]
//! and [`crate::gatekeeper`] do: to reuse this crate's private
//! length-prefixed-JSON framing primitives ([`crate::write_framed`]/
//! [`crate::read_frame_bytes`]) without changing their visibility.
//!
//! A new, separate module rather than new variants on
//! [`crate::extension`]'s `ExtensionRequest`/`ExtensionReply`: folding
//! script's DOM-call traffic in there would force every extension-side
//! match arm to also cover BlueJS's much higher-frequency,
//! language-runtime-shaped requests (property get/set, node creation),
//! which have nothing to do with capability grants.
//!
//! Like [`crate::extension`] (and unlike [`crate::gatekeeper`]'s
//! one-shot connections), a `bluejs` connection is long-lived and
//! stateful, so it gets the same `Hello`-handshake-then-long-lived-
//! connection shape.
//!
//! **Scope of this MVP slice.** [`ScriptRequest`] covers selectors, node
//! creation/tree mutation, text, attributes, `classList`, inline `style`, and
//! the `value`/`checked` element properties. Events and timers use the reverse
//! [`ScriptCommand`] direction: core dispatches input events or polls due
//! timers, then observes their result only at the same completion barrier that
//! commits DOM writes. This keeps core the DOM/default-action/render owner.
//! Likewise, the design doc's batching (`Batch(Vec<DomOp>)`) and
//! shared-memory read fast path are explicitly additive follow-ups on
//! top of this plain request/reply shape, not part of this slice.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// One message the `bluejs` script host sends to `core` over a
/// long-lived connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScriptRequest {
    /// Sent once, first, on a fresh connection -- the same
    /// `Hello`-handshake shape [`crate::extension::ExtensionRequest::Hello`]
    /// uses, layered onto `gatekeeper`'s framing style, per this
    /// module's own docs above.
    Hello,
    /// Sent by BlueJS after one control turn. Core uses this as the turn
    /// boundary: every preceding DOM request has been applied, so it may
    /// proceed to cascade/layout/paint.
    Complete {
        tab_id: u64,
        error: Option<String>,
        outcome: ScriptTurnOutcome,
    },
    /// `document.getElementById(id)`, scoped to one tab.
    GetElementById { tab_id: u64, id: String },
    /// `document.querySelector(selector)`, scoped to one tab.
    QuerySelector { tab_id: u64, selector: String },
    /// `document.querySelectorAll(selector)`, scoped to one tab. The reply
    /// is a snapshot array of opaque node IDs, not a live DOM collection.
    QuerySelectorAll { tab_id: u64, selector: String },
    /// `document.createElement(tag_name)`, scoped to one tab. Creates
    /// the node but does not attach it anywhere -- a script must still
    /// place it with [`ScriptRequest::AppendChild`] or the like.
    CreateElement { tab_id: u64, tag_name: String },
    /// `document.createTextNode(data)`, scoped to one tab.
    CreateTextNode { tab_id: u64, data: String },
    /// `parent.appendChild(child)`, scoped to one tab. Node identity
    /// (`parent`/`child`) is `blueice_dom::NodeId`'s raw value, the
    /// same convention [`crate::ai::AiNode::id`] already uses for the
    /// same reason: this crate doesn't depend on `blueice-dom` for a
    /// plain numeric handle.
    AppendChild {
        tab_id: u64,
        parent: u64,
        child: u64,
    },
    /// `parent.insertBefore(child, reference)`; `reference: None` means
    /// append, matching the DOM API's `null` form.
    InsertBefore {
        tab_id: u64,
        parent: u64,
        child: u64,
        reference: Option<u64>,
    },
    /// `parent.removeChild(child)`; the node remains a valid detached DOM
    /// identity and may be reattached later.
    RemoveChild {
        tab_id: u64,
        parent: u64,
        child: u64,
    },
    /// `node.remove()`; the node remains a valid detached DOM identity.
    Remove { tab_id: u64, node: u64 },
    /// `node.textContent` getter, scoped to one tab.
    GetTextContent { tab_id: u64, node: u64 },
    /// `node.textContent` setter, scoped to one tab.
    SetTextContent {
        tab_id: u64,
        node: u64,
        value: String,
    },
    GetAttribute {
        tab_id: u64,
        node: u64,
        name: String,
    },
    SetAttribute {
        tab_id: u64,
        node: u64,
        name: String,
        value: String,
    },
    RemoveAttribute {
        tab_id: u64,
        node: u64,
        name: String,
    },
    /// `element.innerHTML` getter only; setter intentionally remains outside
    /// the MVP because it needs HTML fragment parsing.
    GetInnerHtml { tab_id: u64, node: u64 },
    GetStyleProperty {
        tab_id: u64,
        node: u64,
        property: String,
    },
    SetStyleProperty {
        tab_id: u64,
        node: u64,
        property: String,
        value: String,
    },
    ClassList {
        tab_id: u64,
        node: u64,
        operation: ScriptClassListOperation,
        class_name: String,
    },
}

/// The narrow, typed class-list mutation vocabulary carried over script IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptClassListOperation {
    Add,
    Remove,
    Toggle,
    Contains,
}

/// Observable result of a completed BlueJS control turn. Keeping this on the
/// completion barrier (rather than adding a second reply direction) makes an
/// event's default action and a timer's paint invalidation atomic with all DOM
/// requests issued by its callbacks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptTurnOutcome {
    /// Whether an event listener called `preventDefault()`.
    pub default_prevented: bool,
    /// Whether at least one due timer callback ran in this turn.
    pub ran_timers: bool,
    /// Whether at least one listener was invoked for a dispatched event.
    pub ran_event: bool,
}

/// A control message sent from core to the always-resident BlueJS process.
/// It deliberately has its own direction-specific type: `ScriptRequest` is
/// a DOM operation BlueJS asks core to perform, while this enum asks BlueJS
/// to execute/reset an isolated tab realm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScriptCommand {
    Execute { tab_id: u64, source: String },
    ResetRealm { tab_id: u64 },
    /// Dispatches one DOM event to listeners registered in the addressed
    /// realm. Core applies the browser default action only when completion
    /// reports that it was not prevented.
    DispatchEvent {
        tab_id: u64,
        node: u64,
        event_type: String,
    },
    /// Runs every expired `setTimeout` callback for one realm. Core polls this
    /// on its existing short session tick, so the script process never needs
    /// a second clock-owning thread or an unsolicited IPC message.
    RunDueTimers { tab_id: u64 },
    Shutdown,
}

/// `core`'s reply to one [`ScriptRequest`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScriptReply {
    /// Reply to [`ScriptRequest::Hello`].
    HelloAck,
    /// Reply to [`ScriptRequest::GetElementById`] -- `None` if no
    /// element with that ID exists in the addressed tab, matching
    /// `document.getElementById`'s own `null`-on-miss behavior rather
    /// than treating a miss as an error.
    Node { node: Option<u64> },
    /// Reply to `querySelectorAll`; ordering is document order.
    Nodes { nodes: Vec<u64> },
    /// Reply to [`ScriptRequest::CreateElement`]/
    /// [`ScriptRequest::CreateTextNode`] -- creation is infallible
    /// given a valid tab, so this carries the new node's ID directly
    /// rather than an `Option`.
    NodeCreated { node: u64 },
    /// Reply to [`ScriptRequest::AppendChild`]/
    /// [`ScriptRequest::SetTextContent`] -- both are plain mutations
    /// with no meaningful success payload.
    Ack,
    /// Reply to [`ScriptRequest::GetTextContent`].
    Text { value: String },
    /// `getAttribute` reply, where `None` is JavaScript `null`.
    Attribute { value: Option<String> },
    /// Reply to boolean-returning DOM operations such as `classList.toggle`.
    Bool { value: bool },
    /// Reply to any request naming a tab or node that doesn't exist --
    /// a structured error, mirroring
    /// [`crate::extension::ExtensionReply::CapabilityDenied`]'s
    /// "structured, not a silent no-op" precedent.
    Error { message: String },
}

pub fn write_script_request<W: Write>(w: &mut W, msg: &ScriptRequest) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_script_request<R: Read>(r: &mut R) -> io::Result<ScriptRequest> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_script_reply<W: Write>(w: &mut W, msg: &ScriptReply) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_script_reply<R: Read>(r: &mut R) -> io::Result<ScriptReply> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn write_script_command<W: Write>(w: &mut W, msg: &ScriptCommand) -> io::Result<()> {
    crate::write_framed(w, msg)
}

pub fn read_script_command<R: Read>(r: &mut R) -> io::Result<ScriptCommand> {
    let buf = crate::read_frame_bytes(r)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// Where `core` listens for a `bluejs` script host connection, and
/// where `bluejs` connects by default in production. Mirrors
/// [`crate::extension::default_extension_socket_path`]'s exact style
/// (same per-user temp dir convention, so two different users -- or two
/// independent BlueIce sessions -- never collide), but a distinct
/// filename. Production code is the only caller that uses this default
/// directly -- tests thread an explicit socket path through instead,
/// for the same reason the other `default_*_socket_path` functions'
/// docs give.
pub fn default_script_socket_path() -> PathBuf {
    crate::local_socket::default_socket_dir().join("bluejs.sock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn script_request_round_trips_over_a_real_socket() {
        for req in [
            ScriptRequest::Hello,
            ScriptRequest::Complete {
                tab_id: 1,
                error: None,
                outcome: ScriptTurnOutcome::default(),
            },
            ScriptRequest::GetElementById {
                tab_id: 1,
                id: "widget".to_string(),
            },
            ScriptRequest::QuerySelector {
                tab_id: 1,
                selector: ".widget".to_string(),
            },
            ScriptRequest::QuerySelectorAll {
                tab_id: 1,
                selector: "li".to_string(),
            },
            ScriptRequest::CreateElement {
                tab_id: 1,
                tag_name: "li".to_string(),
            },
            ScriptRequest::CreateTextNode {
                tab_id: 1,
                data: "hello".to_string(),
            },
            ScriptRequest::AppendChild {
                tab_id: 1,
                parent: 10,
                child: 11,
            },
            ScriptRequest::InsertBefore {
                tab_id: 1,
                parent: 10,
                child: 11,
                reference: None,
            },
            ScriptRequest::RemoveChild {
                tab_id: 1,
                parent: 10,
                child: 11,
            },
            ScriptRequest::Remove {
                tab_id: 1,
                node: 11,
            },
            ScriptRequest::GetTextContent {
                tab_id: 1,
                node: 10,
            },
            ScriptRequest::SetTextContent {
                tab_id: 1,
                node: 10,
                value: "updated".to_string(),
            },
            ScriptRequest::GetAttribute {
                tab_id: 1,
                node: 10,
                name: "class".to_string(),
            },
            ScriptRequest::SetAttribute {
                tab_id: 1,
                node: 10,
                name: "class".to_string(),
                value: "active".to_string(),
            },
            ScriptRequest::RemoveAttribute {
                tab_id: 1,
                node: 10,
                name: "class".to_string(),
            },
            ScriptRequest::GetInnerHtml {
                tab_id: 1,
                node: 10,
            },
            ScriptRequest::GetStyleProperty {
                tab_id: 1,
                node: 10,
                property: "display".to_string(),
            },
            ScriptRequest::SetStyleProperty {
                tab_id: 1,
                node: 10,
                property: "display".to_string(),
                value: "none".to_string(),
            },
            ScriptRequest::ClassList {
                tab_id: 1,
                node: 10,
                operation: ScriptClassListOperation::Toggle,
                class_name: "active".to_string(),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_script_request(&mut a, &req).unwrap();
            assert_eq!(read_script_request(&mut b).unwrap(), req);
        }
    }

    #[test]
    fn script_reply_round_trips_over_a_real_socket() {
        for reply in [
            ScriptReply::HelloAck,
            ScriptReply::Node { node: Some(42) },
            ScriptReply::Node { node: None },
            ScriptReply::Nodes {
                nodes: vec![42, 43],
            },
            ScriptReply::NodeCreated { node: 43 },
            ScriptReply::Ack,
            ScriptReply::Text {
                value: "hello".to_string(),
            },
            ScriptReply::Attribute {
                value: Some("active".to_string()),
            },
            ScriptReply::Attribute { value: None },
            ScriptReply::Bool { value: true },
            ScriptReply::Error {
                message: "no such tab".to_string(),
            },
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_script_reply(&mut a, &reply).unwrap();
            assert_eq!(read_script_reply(&mut b).unwrap(), reply);
        }
    }

    #[test]
    fn script_commands_round_trip_over_the_same_framing_boundary() {
        for command in [
            ScriptCommand::Execute {
                tab_id: 1,
                source: "let x = 1;".to_string(),
            },
            ScriptCommand::ResetRealm { tab_id: 1 },
            ScriptCommand::DispatchEvent {
                tab_id: 1,
                node: 42,
                event_type: "click".to_string(),
            },
            ScriptCommand::RunDueTimers { tab_id: 1 },
            ScriptCommand::Shutdown,
        ] {
            let (mut a, mut b) = UnixStream::pair().unwrap();
            write_script_command(&mut a, &command).unwrap();
            assert_eq!(read_script_command(&mut b).unwrap(), command);
        }
    }

    #[test]
    fn multiple_requests_can_be_written_and_read_in_sequence_on_one_stream() {
        let mut buf = Vec::new();
        write_script_request(
            &mut buf,
            &ScriptRequest::GetElementById {
                tab_id: 1,
                id: "a".to_string(),
            },
        )
        .unwrap();
        write_script_request(
            &mut buf,
            &ScriptRequest::GetElementById {
                tab_id: 1,
                id: "b".to_string(),
            },
        )
        .unwrap();
        let mut cursor = std::io::Cursor::new(buf);
        assert_eq!(
            read_script_request(&mut cursor).unwrap(),
            ScriptRequest::GetElementById {
                tab_id: 1,
                id: "a".to_string()
            }
        );
        assert_eq!(
            read_script_request(&mut cursor).unwrap(),
            ScriptRequest::GetElementById {
                tab_id: 1,
                id: "b".to_string()
            }
        );
    }

    #[test]
    fn default_script_socket_path_is_distinct_from_the_other_well_known_socket_paths() {
        // Same per-user-temp-dir convention the other `default_*_socket_path`
        // functions use, but a distinct filename -- must never resolve to
        // the same path any other protocol's clients connect to.
        let path = default_script_socket_path();
        assert_eq!(path.file_name().unwrap(), "bluejs.sock");
        assert_ne!(path.file_name().unwrap(), "core.sock");
        assert_ne!(path.file_name().unwrap(), "ai-gatekeeper.sock");
        assert_ne!(path.file_name().unwrap(), "extension-host.sock");
    }

    #[test]
    fn reading_malformed_json_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        let bad_payload = b"not json";
        buf.extend_from_slice(&(bad_payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(bad_payload);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_script_request(&mut cursor).is_err());
    }

    #[test]
    fn reading_a_truncated_frame_is_an_error_not_a_panic() {
        let mut buf = Vec::new();
        write_script_reply(&mut buf, &ScriptReply::HelloAck).unwrap();
        buf.truncate(buf.len() - 1);
        let mut cursor = std::io::Cursor::new(buf);
        assert!(read_script_reply(&mut cursor).is_err());
    }
}
