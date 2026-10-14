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
    assert!(receiver.refresh_admission_hold(&tabs).is_none());
    tabs.get_mut(tab)
        .unwrap()
        .load_html_str("<main>one</main>", None);
    assert_eq!(receiver.refresh_admission_hold(&tabs), Some(tab));
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
    assert_eq!(receiver.refresh_admission_hold(&tabs), Some(tab));
    tabs.get_mut(tab)
        .unwrap()
        .load_html_str("<main>two</main>", None);
    assert!(receiver.refresh_admission_hold(&tabs).is_none());

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
    assert!(receiver.refresh_admission_hold(&tabs).is_none());

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
    assert!(receiver.refresh_admission_hold(&tabs).is_none());
}

#[test]
fn reserved_native_tab_does_not_delay_an_unreserved_pending_tab() {
    let mut tabs = TabManager::new(320.0, 200.0);
    let reserved_tab = tabs.default_tab();
    let other_tab = tabs.open_tab();
    for tab in [reserved_tab, other_tab] {
        tabs.get_mut(tab).unwrap().load_html_str(
            "<script>let answer = 42;</script>",
            Some(format!("https://example.test/{}", tab.as_u64())),
        );
    }
    let mut executor = JavaScriptPageExecutor::with_config(
        crate::script::javascript::JavaScriptPageExecutorConfig {
            native_debugger_execution_control: true,
            ..Default::default()
        },
    )
    .unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();
    let program_for = |executor: &mut JavaScriptPageExecutor, tab: TabId| {
        let realm = DebuggerPageRealm {
            browser_context_id: DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: tab.as_u64(),
            realm_generation: 1,
        };
        let DebuggerReply::Programs(programs) = handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(executor),
            DebuggerRequest::ListPrograms { realm },
        ) else {
            panic!("one live program must be discoverable for each tab")
        };
        assert_eq!(programs.len(), 1);
        programs[0]
    };
    let reserved_program = program_for(&mut executor, reserved_tab);
    let other_program = program_for(&mut executor, other_tab);

    PageJavaScriptExecutor::hold_reserved_debugger_execution_once(&mut executor, reserved_tab);
    executor.synchronize_and_execute(&tabs).unwrap();
    for (program, expected) in [
        (reserved_program, DebuggerExecutionState::Pending),
        (other_program, DebuggerExecutionState::Completed),
    ] {
        assert_eq!(
            handle_debugger_request_with_javascript_executor(
                &tabs,
                Some(&mut executor),
                DebuggerRequest::GetExecutionState { program },
            ),
            DebuggerReply::ExecutionState {
                program,
                state: expected,
            }
        );
    }
    executor.synchronize_and_execute(&tabs).unwrap();
    assert_eq!(
        handle_debugger_request_with_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::GetExecutionState {
                program: reserved_program,
            },
        ),
        DebuggerReply::ExecutionState {
            program: reserved_program,
            state: DebuggerExecutionState::Completed,
        }
    );
}

#[test]
fn transition_observation_is_exact_stream_bound_and_bounded() {
    let (mut tabs, realm) = loaded_tabs();
    let tab = TabId::from_u64(realm.tab_id);
    let (_, receiver) = debugger_request_channel();
    let owner = negotiated_session();
    let foreign = negotiated_session();
    let program = DebuggerProgram {
        realm,
        program_handle: 1,
        program_generation: 1,
    };
    assert!(receiver.remember_transition_reply(
        &tabs,
        Some(&owner),
        &DebuggerReply::ExecutionResumed { program },
    ));
    assert_eq!(receiver.refresh_transition_holds(&tabs), vec![tab]);
    let observed = DebuggerReply::ExecutionState {
        program,
        state: DebuggerExecutionState::Resuming,
    };
    receiver.observe_transition_reply(Some(&foreign), &observed);
    assert_eq!(receiver.refresh_transition_holds(&tabs), vec![tab]);
    receiver.observe_transition_reply(
        Some(&owner),
        &DebuggerReply::ExecutionState {
            program: DebuggerProgram {
                program_handle: 9,
                ..program
            },
            state: DebuggerExecutionState::Resuming,
        },
    );
    assert_eq!(receiver.refresh_transition_holds(&tabs), vec![tab]);
    receiver.observe_transition_reply(Some(&owner), &observed);
    assert!(receiver.refresh_transition_holds(&tabs).is_empty());

    let frame = DebuggerLinkedFrame {
        program,
        code_unit_ordinal: 1,
        core_instance: [1; 16],
        frame_handle: 7,
    };
    let entry = DebuggerProgram {
        program_handle: 2,
        program_generation: 2,
        ..program
    };
    assert!(receiver.remember_transition_reply(
        &tabs,
        Some(&owner),
        &DebuggerReply::LinkedNestedResumeRequested { top_frame: frame },
    ));
    receiver.observe_transition_reply(
        Some(&owner),
        &DebuggerReply::LinkedExecutionState {
            entry,
            state: Box::new(DebuggerLinkedExecutionState::Resuming { frame }),
        },
    );
    assert!(receiver.refresh_transition_holds(&tabs).is_empty());

    assert!(receiver.remember_transition_reply(
        &tabs,
        Some(&owner),
        &DebuggerReply::ExecutionResumed { program },
    ));
    tabs.get_mut(tab)
        .unwrap()
        .load_html_str("<main>replacement</main>", None);
    assert!(receiver.refresh_transition_holds(&tabs).is_empty());

    assert!(receiver.remember_transition_reply(
        &tabs,
        Some(&owner),
        &DebuggerReply::ExecutionResumed { program },
    ));
    receiver
        .transition_holds
        .borrow_mut()
        .get_mut(&tab)
        .unwrap()
        .expires_at = Instant::now() - Duration::from_millis(1);
    assert!(receiver.refresh_transition_holds(&tabs).is_empty());

    assert!(receiver.remember_transition_reply(
        &tabs,
        Some(&owner),
        &DebuggerReply::ExecutionResumed { program },
    ));
    drop(owner);
    assert!(receiver.refresh_transition_holds(&tabs).is_empty());
}
