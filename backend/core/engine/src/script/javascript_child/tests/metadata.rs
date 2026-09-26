// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn core_proxies_real_child_debugger_locations_with_core_ids_and_rejects_stale_cross_tab_targets() {
    use crate::debugger::handle_debugger_request_with_page_javascript_executor;
    use blueice_ipc::debugger::{
        DebuggerCapability, DebuggerCapabilityState, DebuggerErrorCode, DebuggerPageRealm,
        DebuggerReply, DebuggerRequest,
    };

    let (path, token, child) = spawn_child();
    let (mut tabs, first_tab) = loaded_tabs(
        "<script>let first = 1; first += 1;</script>",
        "https://example.test/first.html",
    );
    let second_tab = tabs.open_tab();
    tabs.get_mut(second_tab).unwrap().load_html_str(
        "<script>let second = 2;</script>",
        Some("https://example.test/second.html".to_string()),
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();

    let first_realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: first_tab.as_u64(),
        realm_generation: 1,
    };
    let second_realm = DebuggerPageRealm {
        browser_context_id: crate::debugger::DEFAULT_BROWSER_CONTEXT_ID,
        tab_id: second_tab.as_u64(),
        realm_generation: 1,
    };
    let capabilities = handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::DescribeCapabilities { realm: first_realm },
    );
    let DebuggerReply::Capabilities(capabilities) = capabilities else {
        panic!("expected child debugger capabilities");
    };
    assert_eq!(
        capabilities
            .reports
            .iter()
            .find(|report| report.capability == DebuggerCapability::ProgramLocations)
            .map(|report| report.state),
        Some(DebuggerCapabilityState::Available)
    );
    assert_eq!(
        capabilities
            .reports
            .iter()
            .find(|report| report.capability == DebuggerCapability::BreakpointConfiguration)
            .map(|report| report.state),
        Some(DebuggerCapabilityState::Available)
    );
    assert!(
        capabilities
            .reports
            .iter()
            .all(|report| !report.detail.contains("first") && !report.detail.contains("bytecode")),
        "capability replies must remain source/bytecode-free"
    );

    let programs = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListPrograms { realm: first_realm },
    ) {
        DebuggerReply::Programs(programs) => programs,
        reply => panic!("expected public program inventory, got {reply:?}"),
    };
    let program = *programs.first().expect("first child page has one program");
    assert!(
        program.program_handle >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
            && program.program_generation >= CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
        "core must mint a public namespace instead of forwarding child IDs"
    );
    let safe_points = match handle_debugger_request_with_page_javascript_executor(
        &tabs,
        Some(&mut executor),
        DebuggerRequest::ListSafePoints { program },
    ) {
        DebuggerReply::SafePoints(safe_points) => safe_points,
        reply => panic!("expected source-free public safe points, got {reply:?}"),
    };
    let safe_point = *safe_points.first().expect("program exposes one safe point");
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ValidateSafePoint { safe_point },
        ),
        DebuggerReply::SafePointValidated { safe_point }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );
    // Configuration is idempotent and does not create another public or
    // child-private record on a debugger socket retry.
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListBreakpoints { realm: first_realm },
        ),
        DebuggerReply::Breakpoints(vec![safe_point])
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ClearBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: true,
        }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ClearBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointCleared {
            safe_point,
            was_present: false,
        }
    );
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint { safe_point },
        ),
        DebuggerReply::BreakpointSet { safe_point }
    );

    let cross_tab = blueice_ipc::debugger::DebuggerProgram {
        realm: second_realm,
        ..program
    };
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints { program: cross_tab },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    let cross_tab_safe_point = blueice_ipc::debugger::DebuggerSafePoint {
        program: cross_tab,
        ..safe_point
    };
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::SetBreakpoint {
                safe_point: cross_tab_safe_point,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));

    // Reconfiguration before navigation proves that the successor's
    // empty child table cannot retain prior private IDs or public tuples.
    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<script>let successor = 4;</script>",
        Some("https://example.test/successor-again.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    let successor_realm = DebuggerPageRealm {
        realm_generation: 2,
        ..first_realm
    };
    assert_eq!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListBreakpoints {
                realm: successor_realm,
            },
        ),
        DebuggerReply::Breakpoints(Vec::new())
    );

    tabs.get_mut(first_tab).unwrap().load_html_str(
        "<script>let successor = 3;</script>",
        Some("https://example.test/successor.html".to_string()),
    );
    assert!(matches!(
        handle_debugger_request_with_page_javascript_executor(
            &tabs,
            Some(&mut executor),
            DebuggerRequest::ListSafePoints { program },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::StaleRealm,
            ..
        }
    ));

    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}

#[test]
fn core_remints_real_child_bluets_metadata_handles_and_discards_them_on_navigation() {
    let (path, token, child) = spawn_child();
    let (mut tabs, tab_id) = loaded_tabs(
        concat!(
            "<script>globalThis.javaScriptOnly = true;</script>",
            "<script type=\"application/x-blueice-typescript\">",
            "const opaqueCompilerMetadata: number = 42;",
            "</script>"
        ),
        "https://example.test/opaque-metadata.html",
    );
    let mut executor = OutOfProcessJavaScriptPageExecutor::connect(&path, &token).unwrap();
    executor.synchronize_and_execute(&tabs).unwrap();

    let programs = executor.debugger_programs(tab_id, 1).unwrap();
    assert_eq!(programs.len(), 2);
    let mut metadata = None;
    for program in programs {
        let handles = executor
            .debugger_static_metadata(
                tab_id,
                1,
                program.program_handle,
                program.program_generation,
            )
            .unwrap();
        if let [handle] = handles.as_slice() {
            assert!(
                handle.metadata_handle >= CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
                    && handle.metadata_handle < CORE_CHILD_DEBUGGER_ID_NAMESPACE_START
                    && handle.metadata_generation
                        >= CORE_CHILD_DEBUGGER_METADATA_ID_NAMESPACE_START
                    && handle.metadata_generation < CORE_CHILD_DEBUGGER_ID_NAMESPACE_START,
                "core must remint metadata IDs outside both child and public program namespaces"
            );
            metadata = Some((program, *handle));
        } else {
            assert!(handles.is_empty(), "the JavaScript program is ineligible");
        }
    }
    let (typed_program, metadata) = metadata.expect("direct BlueTS has one private attachment");
    assert!(
        !format!("{metadata:?}").contains("opaqueCompilerMetadata"),
        "the core-facing handle must contain no compiler metadata payload"
    );
    let summary = executor
        .debugger_static_metadata_summary(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        )
        .expect("the exact core-reminted metadata identity resolves a bounded summary");
    assert_eq!(summary.language_version, "blue-ts-0.1");
    assert!(summary.source_count > 0);
    assert!(summary.type_count > 0);
    assert!(summary.symbol_count > 0);
    assert!(
        !format!("{summary:?}").contains("opaqueCompilerMetadata"),
        "the core-facing summary must contain no compiler record payload"
    );
    let sources = executor
        .debugger_static_metadata_sources(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        )
        .expect("the exact core-reminted metadata identity resolves source-record IDs");
    assert_eq!(
        sources.len(),
        usize::try_from(summary.source_count).unwrap()
    );
    assert_eq!(
        sources
            .iter()
            .map(|source| source.source_id)
            .collect::<BTreeSet<_>>()
            .len(),
        sources.len(),
        "the child must not repeat compiler source-record IDs"
    );
    assert!(
        !format!("{sources:?}").contains("opaqueCompilerMetadata"),
        "source-record identities must not carry compiler record payloads"
    );
    assert!(matches!(
        executor.debugger_static_metadata_sources(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation + 1,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    ));
    assert!(matches!(
        executor.debugger_static_metadata_summary(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation + 1,
        ),
        Err(JavaScriptPageDebuggerError::UnknownProgram)
    ));

    tabs.get_mut(tab_id).unwrap().load_html_str(
        "<script type=\"application/x-blueice-typescript\">const successor: number = 1;</script>",
        Some("https://example.test/opaque-metadata-successor.html".to_string()),
    );
    executor.synchronize_and_execute(&tabs).unwrap();
    assert!(matches!(
        executor.debugger_static_metadata(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));
    assert!(matches!(
        executor.debugger_static_metadata_summary(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));
    assert!(matches!(
        executor.debugger_static_metadata_sources(
            tab_id,
            1,
            typed_program.program_handle,
            typed_program.program_generation,
            metadata.metadata_handle,
            metadata.metadata_generation,
        ),
        Err(JavaScriptPageDebuggerError::NoLiveRealm)
    ));

    drop(executor);
    shutdown_child(&path, &token);
    child.join().unwrap();
    let _ = std::fs::remove_file(path);
}
