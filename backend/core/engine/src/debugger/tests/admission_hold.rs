// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use blueice_ipc::debugger::{metadata_session_authorization, negotiate_with_values};

fn negotiated_session() -> DebuggerMetadataSessionAuthorization {
    let hello = DebuggerRequest::Hello {
        protocol_version: DEBUGGER_PROTOCOL_VERSION,
        requested_metadata_capabilities: Default::default(),
        requested_bounded_values: false,
    };
    let reply = negotiate_with_values(&hello, &Default::default(), false);
    metadata_session_authorization(&hello, &reply).unwrap()
}

fn dispatch(
    sender: &DebuggerRequestSender,
    receiver: &DebuggerRequestReceiver,
    tabs: &TabManager,
    request: DebuggerRequest,
    session: Option<&DebuggerMetadataSessionAuthorization>,
) -> DebuggerReply {
    let (reply_sender, reply_receiver) = mpsc::sync_channel(1);
    sender
        .0
        .send(DebuggerRequestEnvelope {
            request,
            metadata_session: session.cloned(),
            reply: reply_sender,
        })
        .unwrap();
    assert_eq!(receiver.dispatch_pending(tabs, None), 1);
    reply_receiver.recv().unwrap()
}

#[test]
fn next_document_hold_requires_a_live_owner_stream_and_exact_tab() {
    let tabs = TabManager::new(320.0, 200.0);
    let tab_id = tabs.default_tab().as_u64();
    let (sender, receiver) = debugger_request_channel();
    let owner = negotiated_session();
    let foreign = negotiated_session();

    assert!(matches!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::HoldNextDocument { tab_id },
            None,
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    assert!(matches!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::HoldNextDocument { tab_id: 999 },
            Some(&owner),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert_eq!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::HoldNextDocument { tab_id },
            Some(&owner),
        ),
        DebuggerReply::NextDocumentHoldAcquired { tab_id }
    );
    assert!(matches!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::ReleaseNextDocumentHold { tab_id },
            Some(&foreign),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert!(matches!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::HoldNextDocument { tab_id },
            Some(&foreign),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::ResourceLimit,
            ..
        }
    ));
    assert_eq!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::ReleaseNextDocumentHold { tab_id },
            Some(&owner),
        ),
        DebuggerReply::NextDocumentHoldReleased { tab_id }
    );
}

#[test]
fn next_document_hold_attaches_once_and_expires_on_replacement_disconnect_or_timeout() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let tab = tabs.default_tab();
    let tab_id = tab.as_u64();
    let (sender, receiver) = debugger_request_channel();
    let owner = negotiated_session();
    assert_eq!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::HoldNextDocument { tab_id },
            Some(&owner),
        ),
        DebuggerReply::NextDocumentHoldAcquired { tab_id }
    );
    assert!(!receiver.refresh_admission_hold(&tabs));
    tabs.get_mut(tab)
        .unwrap()
        .load_html_str("<main>one</main>", None);
    assert!(receiver.refresh_admission_hold(&tabs));
    let foreign = negotiated_session();
    let safe_point = DebuggerSafePoint {
        program: DebuggerProgram {
            realm: DebuggerPageRealm {
                browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
                tab_id,
                realm_generation: 1,
            },
            program_handle: 1,
            program_generation: 1,
        },
        code_unit_ordinal: 0,
        bytecode_offset: 0,
    };
    assert!(matches!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::ArmEntryBreakpoint { safe_point },
            Some(&foreign),
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert!(receiver.refresh_admission_hold(&tabs));
    tabs.get_mut(tab)
        .unwrap()
        .load_html_str("<main>two</main>", None);
    assert!(!receiver.refresh_admission_hold(&tabs));

    assert_eq!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::HoldNextDocument { tab_id },
            Some(&owner),
        ),
        DebuggerReply::NextDocumentHoldAcquired { tab_id }
    );
    drop(owner);
    assert!(!receiver.refresh_admission_hold(&tabs));

    let owner = negotiated_session();
    assert_eq!(
        dispatch(
            &sender,
            &receiver,
            &tabs,
            DebuggerRequest::HoldNextDocument { tab_id },
            Some(&owner),
        ),
        DebuggerReply::NextDocumentHoldAcquired { tab_id }
    );
    receiver
        .admission_hold
        .borrow_mut()
        .as_mut()
        .unwrap()
        .expires_at = Instant::now() - Duration::from_millis(1);
    assert!(!receiver.refresh_admission_hold(&tabs));
}
