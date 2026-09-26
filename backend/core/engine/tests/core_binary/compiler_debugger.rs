// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn real_subprocess_serves_only_core_registered_compiler_queries_through_its_session() {
    // This exercises the public process seam rather than adapter methods: the
    // core owner selects a compiled-in closed project before binding the
    // compiler socket, the listener negotiates on its worker, and the later
    // query only completes after the live core session dispatches it.
    let socket_path = unique_socket_path("compiler-core");
    let compiler_socket_path = unique_socket_path("compiler-service");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-compiler-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&compiler_socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--compiler-socket",
            compiler_socket_path.to_str().unwrap(),
            "--compiler-project-profile",
            "core-closed-fixture-v1",
            "--frame-dir",
            frame_dir.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core with its compiler listener");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    assert!(wait_for(&compiler_socket_path, Duration::from_secs(5)));
    assert_eq!(
        std::fs::metadata(&compiler_socket_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "compiler metadata socket must not rely on the ambient umask"
    );
    let mut frontend = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();

    // A query before Hello is rejected at the listener and cannot reach the
    // catalog. This request supplies only an opaque numeric ID, never source
    // or any registration field.
    let mut invalid = connect_with_retry(&compiler_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::compiler::write_compiler_request(
        &mut invalid,
        &blueice_ipc::compiler::CompilerRequest::Check {
            project: blueice_ipc::compiler::CompilerProject { id: 1 },
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::compiler::read_compiler_reply(&mut invalid).unwrap(),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::ProtocolVersion,
            ..
        }
    ));
    drop(invalid);

    // Obsolete compiler IPC versions cannot silently negotiate with the v5
    // core-minted stream attestation and fixed query-only manifest.
    let mut v1 = connect_with_retry(&compiler_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::compiler::write_compiler_request(
        &mut v1,
        &blueice_ipc::compiler::CompilerRequest::Hello {
            protocol_version: 1,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::compiler::read_compiler_reply(&mut v1).unwrap(),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::ProtocolVersion,
            ..
        }
    ));
    drop(v1);

    let mut v2 = connect_with_retry(&compiler_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::compiler::write_compiler_request(
        &mut v2,
        &blueice_ipc::compiler::CompilerRequest::Hello {
            protocol_version: 2,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::compiler::read_compiler_reply(&mut v2).unwrap(),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::ProtocolVersion,
            ..
        }
    ));
    drop(v2);

    let mut v3 = connect_with_retry(&compiler_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::compiler::write_compiler_request(
        &mut v3,
        &blueice_ipc::compiler::CompilerRequest::Hello {
            protocol_version: 3,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::compiler::read_compiler_reply(&mut v3).unwrap(),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::ProtocolVersion,
            ..
        }
    ));
    drop(v3);

    let mut compiler = connect_with_retry(&compiler_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::Hello {
            protocol_version: blueice_ipc::compiler::COMPILER_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let blueice_ipc::compiler::CompilerReply::HelloAck {
        protocol_version,
        session_attestation,
        capability_manifest,
    } = blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap()
    else {
        panic!("core compiler listener must mint attestation and capability manifest")
    };
    assert_eq!(
        protocol_version,
        blueice_ipc::compiler::COMPILER_PROTOCOL_VERSION
    );
    assert!(session_attestation.is_well_formed());
    assert!(capability_manifest.is_well_formed());
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::DescribeProject {
            project: blueice_ipc::compiler::CompilerProject { id: 1 },
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap(),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::UnobservedProject,
            ..
        }
    ));
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::ListProjects,
    )
    .unwrap();
    let blueice_ipc::compiler::CompilerReply::Projects(projects) =
        blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap()
    else {
        panic!("accepted compiler stream must receive sealed project inventory")
    };
    assert_eq!(
        projects.projects,
        vec![blueice_ipc::compiler::CompilerProject { id: 1 }]
    );
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::DescribeProject {
            project: blueice_ipc::compiler::CompilerProject { id: 1 },
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap(),
        blueice_ipc::compiler::CompilerReply::Project(
            blueice_ipc::compiler::CompilerProjectIdentity {
                project: blueice_ipc::compiler::CompilerProject { id: 1 },
                entry_module: "project:///core-fixture/main.ts".to_string(),
            }
        )
    );
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::Check {
            project: blueice_ipc::compiler::CompilerProject { id: 1 },
        },
    )
    .unwrap();
    let blueice_ipc::compiler::CompilerReply::Check(check) =
        blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap()
    else {
        panic!("core-registered fixture must return a bounded compiler check")
    };
    assert!(!check.has_errors, "{check:#?}");
    assert_eq!(check.generation.project.id, 1);
    assert_eq!(check.static_metadata.as_ref().unwrap().contract_count, 1);
    assert!(check.artifact_fingerprint.is_some());

    // The public process route first returns an opaque bounded inventory. The
    // caller discovers the static symbol ID from this exact check generation
    // rather than guessing an ordinal, then follows its source/contract IDs.
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: blueice_ipc::compiler::CompilerStaticMetadataKind::Symbols,
            cursor: None,
            limit: Some(128),
        },
    )
    .unwrap();
    let blueice_ipc::compiler::CompilerReply::StaticMetadataPage(symbol_ids) =
        blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap()
    else {
        panic!("real core must provide bounded exact-generation symbol IDs")
    };
    assert!(symbol_ids.next_cursor.is_none());
    assert_eq!(
        u32::try_from(symbol_ids.ids.len()).unwrap(),
        check.static_metadata.as_ref().unwrap().symbol_count
    );
    let symbol_id = *symbol_ids
        .ids
        .first()
        .expect("compiled-in interface must be discoverable in the inventory");
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::GetStaticSymbol {
            generation: check.generation,
            symbol_id,
        },
    )
    .unwrap();
    let blueice_ipc::compiler::CompilerReply::StaticSymbol(symbol) =
        blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap()
    else {
        panic!("compiled-in interface must expose exact static metadata")
    };
    let contract_id = symbol
        .contract_id
        .expect("local interface must have retained plan");
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::GetStaticProvenance {
            generation: check.generation,
            source_id: symbol.source_id,
        },
    )
    .unwrap();
    let blueice_ipc::compiler::CompilerReply::StaticProvenance(provenance) =
        blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap()
    else {
        panic!("real core must return only source-free provenance")
    };
    assert_ne!(provenance.content_hash, "CoreFixtureSettings");
    assert!(provenance.content_hash.starts_with("bts-sha256:"));
    assert_eq!(provenance.content_hash.len(), "bts-sha256:".len() + 64);
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::GetStaticContract {
            generation: check.generation,
            contract_id,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap(),
        blueice_ipc::compiler::CompilerReply::StaticContract(_)
    ));
    let secret = "caller-secret-must-not-be-echoed";
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::ValidateStaticContract {
            generation: check.generation,
            contract_id,
            value: blueice_ipc::compiler::CompilerContractValue::Object(
                std::collections::BTreeMap::from([(
                    "enabled".to_string(),
                    blueice_ipc::compiler::CompilerContractValue::String(secret.to_string()),
                )]),
            ),
        },
    )
    .unwrap();
    let validation = blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap();
    assert!(matches!(
        validation,
        blueice_ipc::compiler::CompilerReply::ContractValidation(
            blueice_ipc::compiler::CompilerContractValidation { valid: false, .. }
        )
    ));
    assert!(!format!("{validation:?}").contains(secret));
    blueice_ipc::compiler::write_compiler_request(
        &mut compiler,
        &blueice_ipc::compiler::CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: blueice_ipc::compiler::CompilerStaticMetadataKind::Symbols,
            cursor: None,
            limit: Some(1),
        },
    )
    .unwrap();
    let blueice_ipc::compiler::CompilerReply::StaticMetadataPage(first_page) =
        blueice_ipc::compiler::read_compiler_reply(&mut compiler).unwrap()
    else {
        panic!("the first stream must receive a bounded symbol page")
    };
    let old_cursor = first_page
        .next_cursor
        .expect("the closed fixture has multiple symbols");
    drop(compiler);

    // The listener accepts only one stream at a time, but a cursor abandoned
    // by that stream must not become usable (or retain a cursor slot) on the
    // next accepted, separately attested stream.
    let mut successor = connect_with_retry(&compiler_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::compiler::write_compiler_request(
        &mut successor,
        &blueice_ipc::compiler::CompilerRequest::Hello {
            protocol_version: blueice_ipc::compiler::COMPILER_PROTOCOL_VERSION,
        },
    )
    .unwrap();
    let blueice_ipc::compiler::CompilerReply::HelloAck {
        session_attestation: successor_attestation,
        ..
    } = blueice_ipc::compiler::read_compiler_reply(&mut successor).unwrap()
    else {
        panic!("the successor stream must receive its own core attestation")
    };
    assert_ne!(successor_attestation, session_attestation);
    blueice_ipc::compiler::write_compiler_request(
        &mut successor,
        &blueice_ipc::compiler::CompilerRequest::ListProjects,
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::compiler::read_compiler_reply(&mut successor).unwrap(),
        blueice_ipc::compiler::CompilerReply::Projects(_)
    ));
    blueice_ipc::compiler::write_compiler_request(
        &mut successor,
        &blueice_ipc::compiler::CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: blueice_ipc::compiler::CompilerStaticMetadataKind::Symbols,
            cursor: Some(old_cursor),
            limit: Some(1),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::compiler::read_compiler_reply(&mut successor).unwrap(),
        blueice_ipc::compiler::CompilerReply::Error {
            code: blueice_ipc::compiler::CompilerErrorCode::InvalidMetadataCursor,
            ..
        }
    ));
    blueice_ipc::compiler::write_compiler_request(
        &mut successor,
        &blueice_ipc::compiler::CompilerRequest::ListStaticMetadata {
            generation: check.generation,
            kind: blueice_ipc::compiler::CompilerStaticMetadataKind::Symbols,
            cursor: None,
            limit: Some(1),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::compiler::read_compiler_reply(&mut successor).unwrap(),
        blueice_ipc::compiler::CompilerReply::StaticMetadataPage(_)
    ));
    drop(successor);

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    let status = child.wait().expect("failed to wait for blueice-core");
    assert!(
        status.success(),
        "compiler core must exit cleanly: {status}"
    );
    assert!(!compiler_socket_path.exists());
    assert!(!frame_dir.exists());
}

#[test]
fn real_subprocess_routes_exact_debugger_locations_through_the_live_core_session() {
    // The debugger protocol must remain separate from both frontend and DOM
    // script IPC, but it still has to validate a target against the session's
    // actual document lifecycle. Exact locations, root interruption, and
    // classic-root stepping are installed only for the explicit in-process
    // JavaScript host; nested frames and VM inspection remain planned.
    let socket_path = unique_socket_path("debugger-core");
    let debugger_socket_path = unique_socket_path("debugger-host");
    let frame_dir = std::env::temp_dir().join(format!(
        "blueice-core-binary-test-debugger-frames-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&debugger_socket_path);
    let _ = std::fs::remove_dir_all(&frame_dir);
    let gatekeeper_path = clearing_gatekeeper("dbg-gk");

    let listener_one = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_one = listener_one.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_one.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>first debugger document</main>",
            "<script>const firstDebuggerLocation = 42;</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let listener_two = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr_two = listener_two.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener_two.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let body = concat!(
            "<main>replacement debugger document</main>",
            "<script>const replacementDebuggerLocation = 43;</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_blueice-core"))
        .args([
            "--socket",
            socket_path.to_str().unwrap(),
            "--debugger-socket",
            debugger_socket_path.to_str().unwrap(),
            "--frame-dir",
            frame_dir.to_str().unwrap(),
            "--gatekeeper-socket",
            gatekeeper_path.to_str().unwrap(),
            "--inline-bluejs",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn blueice-core");

    assert!(wait_for(&socket_path, Duration::from_secs(5)));
    assert!(wait_for(&debugger_socket_path, Duration::from_secs(5)));
    let mut frontend = connect_with_retry(&socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::client_handshake(&mut frontend).unwrap();
    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr_one}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 1, .. }
    ));

    let mut invalid_debugger =
        connect_with_retry(&debugger_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::debugger::write_debugger_request(
        &mut invalid_debugger,
        &blueice_ipc::debugger::DebuggerRequest::DescribeCapabilities {
            realm: blueice_ipc::debugger::DebuggerPageRealm {
                browser_context_id: blueice_engine::debugger::DEFAULT_BROWSER_CONTEXT_ID,
                tab_id: 1,
                realm_generation: 1,
            },
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut invalid_debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::ProtocolVersion,
            ..
        }
    ));
    drop(invalid_debugger);

    let mut debugger = connect_with_retry(&debugger_socket_path, Duration::from_secs(5)).unwrap();
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::Hello {
            protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
            requested_bounded_values: false,
            requested_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty(),
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::HelloAck {
            protocol_version: blueice_ipc::debugger::DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities:
                blueice_ipc::debugger::DebuggerMetadataCapabilityManifest::empty(),
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPageRealms,
    )
    .unwrap();
    let first_realm = match blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap() {
        blueice_ipc::debugger::DebuggerReply::PageRealms(realms) => {
            assert_eq!(realms.len(), 1);
            realms[0]
        }
        other => panic!("expected debugger page realms, got {other:?}"),
    };
    assert_eq!(
        first_realm,
        blueice_ipc::debugger::DebuggerPageRealm {
            browser_context_id: blueice_engine::debugger::DEFAULT_BROWSER_CONTEXT_ID,
            tab_id: 1,
            realm_generation: 1,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::DescribeCapabilities { realm: first_realm },
    )
    .unwrap();
    let first_capabilities =
        match blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap() {
            blueice_ipc::debugger::DebuggerReply::Capabilities(capabilities) => capabilities,
            other => panic!("expected debugger capabilities, got {other:?}"),
        };
    assert_eq!(first_capabilities.realm, first_realm);
    assert!(first_capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::ProgramLocations
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
    }));
    assert!(first_capabilities.reports.iter().any(|report| {
        report.capability == blueice_ipc::debugger::DebuggerCapability::BreakpointConfiguration
            && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
            && report.detail.contains("does not interrupt execution")
    }));
    for capability in [
        blueice_ipc::debugger::DebuggerCapability::Breakpoints,
        blueice_ipc::debugger::DebuggerCapability::PauseResume,
        blueice_ipc::debugger::DebuggerCapability::Stepping,
    ] {
        assert!(first_capabilities.reports.iter().any(|report| {
            report.capability == capability
                && report.state == blueice_ipc::debugger::DebuggerCapabilityState::Available
        }));
    }
    assert!(first_capabilities
        .reports
        .iter()
        .filter(|report| {
            !matches!(
                report.capability,
                blueice_ipc::debugger::DebuggerCapability::ProgramLocations
                    | blueice_ipc::debugger::DebuggerCapability::BreakpointConfiguration
                    | blueice_ipc::debugger::DebuggerCapability::Breakpoints
                    | blueice_ipc::debugger::DebuggerCapability::PauseResume
                    | blueice_ipc::debugger::DebuggerCapability::Stepping
            )
        })
        .all(|report| { report.state == blueice_ipc::debugger::DebuggerCapabilityState::Planned }));

    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPrograms { realm: first_realm },
    )
    .unwrap();
    let first_program = match blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap() {
        blueice_ipc::debugger::DebuggerReply::Programs(programs) => {
            assert_eq!(programs.len(), 1);
            programs[0]
        }
        other => panic!("expected opaque debugger programs, got {other:?}"),
    };
    assert_eq!(first_program.realm, first_realm);
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListSafePoints {
            program: first_program,
        },
    )
    .unwrap();
    let first_safe_point = match blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap()
    {
        blueice_ipc::debugger::DebuggerReply::SafePoints(safe_points) => safe_points
            .into_iter()
            .find(|safe_point| safe_point.code_unit_ordinal == 0 && safe_point.bytecode_offset == 0)
            .expect("the compiled page program has a root entry safe point"),
        other => panic!("expected verified debugger safe points, got {other:?}"),
    };
    assert_eq!(first_safe_point.program, first_program);
    // The core process has not entered this program yet: with the debugger
    // socket selected, its page scheduler gives this handshaken peer one turn
    // to arm the compiler-verified root instruction boundary.
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ArmEntryBreakpoint {
            safe_point: first_safe_point,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::BreakpointArmed {
            safe_point: first_safe_point,
        }
    );
    wait_for_debugger_state(
        &mut debugger,
        first_program,
        blueice_ipc::debugger::DebuggerExecutionState::Paused {
            safe_point: first_safe_point,
        },
        "firstDebuggerLocation",
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ValidateSafePoint {
            safe_point: first_safe_point,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::SafePointValidated {
            safe_point: first_safe_point,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::StepRootInstruction {
            program: first_program,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::ExecutionStepRequested {
            program: first_program,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::GetExecutionState {
            program: first_program,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program: first_program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Stepping,
        }
    );
    let mut stepped_safe_point = None;
    for _ in 0..20 {
        let reply = debugger_request(
            &mut debugger,
            &blueice_ipc::debugger::DebuggerRequest::GetExecutionState {
                program: first_program,
            },
            "firstDebuggerLocation",
        );
        if let blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Paused { safe_point },
        } = reply
        {
            if program == first_program {
                stepped_safe_point = Some(safe_point);
                break;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    let stepped_safe_point =
        stepped_safe_point.expect("one root step must pause at its verified successor");
    assert_ne!(stepped_safe_point, first_safe_point);
    assert_eq!(stepped_safe_point.program, first_program);
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ResumeExecution {
            program: first_program,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::ExecutionResumed {
            program: first_program,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::GetExecutionState {
            program: first_program,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::ExecutionState {
            program: first_program,
            state: blueice_ipc::debugger::DebuggerExecutionState::Resuming,
        }
    );
    // The source-free transition is observable for one owner turn; then the
    // scheduler completes the retained frame without a timing assumption.
    wait_for_debugger_state(
        &mut debugger,
        first_program,
        blueice_ipc::debugger::DebuggerExecutionState::Completed,
        "firstDebuggerLocation",
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::SetBreakpoint {
            safe_point: first_safe_point,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::BreakpointSet {
            safe_point: first_safe_point,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListBreakpoints { realm: first_realm },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Breakpoints(vec![first_safe_point])
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ClearBreakpoint {
            safe_point: first_safe_point,
        },
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::BreakpointCleared {
            safe_point: first_safe_point,
            was_present: true,
        }
    );
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ValidateSafePoint {
            safe_point: blueice_ipc::debugger::DebuggerSafePoint {
                bytecode_offset: u32::MAX,
                ..first_safe_point
            },
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::InvalidSafePoint,
            ..
        }
    ));

    // Keep one configuration record until navigation so the next request
    // proves it cannot survive the document-generation transition.
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::SetBreakpoint {
            safe_point: first_safe_point,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::BreakpointSet { .. }
    ));

    blueice_ipc::write_client_message(
        &mut frontend,
        &blueice_ipc::ClientMessage::Navigate {
            url: format!("http://{addr_two}"),
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::Navigated { .. }
    ));
    assert!(matches!(
        blueice_ipc::read_server_message(&mut frontend).unwrap(),
        blueice_ipc::ServerMessage::FrameReady { generation: 2, .. }
    ));

    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListPageRealms,
    )
    .unwrap();
    assert_eq!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::PageRealms(vec![
            blueice_ipc::debugger::DebuggerPageRealm {
                realm_generation: 2,
                ..first_realm
            },
        ])
    );

    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListSafePoints {
            program: first_program,
        },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::StaleRealm,
            ..
        }
    ));
    blueice_ipc::debugger::write_debugger_request(
        &mut debugger,
        &blueice_ipc::debugger::DebuggerRequest::ListBreakpoints { realm: first_realm },
    )
    .unwrap();
    assert!(matches!(
        blueice_ipc::debugger::read_debugger_reply(&mut debugger).unwrap(),
        blueice_ipc::debugger::DebuggerReply::Error {
            code: blueice_ipc::debugger::DebuggerErrorCode::StaleRealm,
            ..
        }
    ));

    blueice_ipc::write_client_message(&mut frontend, &blueice_ipc::ClientMessage::Shutdown)
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(!socket_path.exists());
    assert!(!debugger_socket_path.exists());
    assert!(!frame_dir.exists());
}
