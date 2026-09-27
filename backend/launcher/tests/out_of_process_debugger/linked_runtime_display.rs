// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Real Launcher sockets for independently granted linked type and value views.

use super::static_runtime_display::{static_manifest, static_receipts};
use super::*;
use blueice_ipc::debugger::{
    DebuggerLinkedScopeTarget, DebuggerLinkedStackSnapshot, DebuggerStaticMetadataTypeId,
    DebuggerStaticScopeTarget, DebuggerValuePreview,
};

const DOCUMENT: &str =
    "<script type=\"application/x-blueice-typescript-module\" src=\"/linked-entry.ts\"></script>";
const ENTRY_SOURCE: &str = "import { inner } from './linked-dependency.ts'; export const rootValue: number = 9; export const answer: number = inner() + rootValue;";
const ENTRY_INTEGRITY: &str =
    "sha256:d6414cb9d7ca28911eb6a2744423f5b6557432ea9d598a3893affe34e6b36217";
const DEPENDENCY_INTEGRITY: &str =
    "sha256:cb66277fc65ebe92e842330a18b6dc5dfba68a2a09a5e2c546eafdb835830d9d";

struct LinkedRuntimeFixture {
    origin: String,
    policy_file: PathBuf,
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<Vec<String>>>,
}

impl LinkedRuntimeFixture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let policy = OwnerHttpPolicyBootstrap {
            origin_rule: OwnerHttpOriginRule::SameDocumentOrigin,
            resources: vec![
                OwnerHttpResource {
                    canonical_url: format!("{origin}/linked-entry.ts"),
                    integrity: ENTRY_INTEGRITY.into(),
                },
                OwnerHttpResource {
                    canonical_url: format!("{origin}/linked-dependency.ts"),
                    integrity: DEPENDENCY_INTEGRITY.into(),
                },
            ],
        };
        let policy_file = unique_path("linked-runtime-policy");
        std::fs::write(&policy_file, serde_json::to_vec(&policy).unwrap()).unwrap();
        let (stop, stopped) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut requested = Vec::new();
            while matches!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                let Ok((mut stream, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0_u8; 2048];
                let count = stream.read(&mut request).unwrap();
                let path = std::str::from_utf8(&request[..count])
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let (mime, body) = match path {
                    "/" => ("text/html", DOCUMENT),
                    "/linked-entry.ts" => ("text/typescript", ENTRY_SOURCE),
                    "/linked-dependency.ts" => ("text/typescript", LINKED_DEPENDENCY_SOURCE),
                    other => panic!("unexpected linked runtime resource {other}"),
                };
                requested.push(path.to_string());
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            }
            requested
        });
        Self {
            origin,
            policy_file,
            stop,
            worker: Some(worker),
        }
    }

    fn finish(&mut self) -> Vec<String> {
        self.stop.send(()).unwrap();
        self.worker.take().unwrap().join().unwrap()
    }
}

impl Drop for LinkedRuntimeFixture {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = self.stop.send(());
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&self.policy_file);
    }
}

fn linked_scopes(
    debugger: &mut UnixStream,
    stack: blueice_ipc::debugger::DebuggerLinkedStackSnapshot,
    max_scope_entries: u32,
) -> blueice_ipc::debugger::DebuggerLinkedScopeSnapshot {
    let DebuggerReply::LinkedScopes(snapshot) = debugger_request(
        debugger,
        DebuggerRequest::GetLinkedScopes {
            expected_stack: stack,
            max_scope_entries,
        },
    ) else {
        panic!("linked entry-root slots must be available on the active stack")
    };
    assert_eq!(snapshot.stack, stack);
    assert_eq!(snapshot.frame_index, 1);
    *snapshot
}

fn timed_debugger(
    socket: &Path,
    bounded_values: bool,
    manifest: DebuggerMetadataCapabilityManifest,
) -> UnixStream {
    let mut debugger = UnixStream::connect(socket).unwrap();
    debugger
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: bounded_values,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: bounded_values,
            granted_metadata_capabilities: manifest,
        }
    );
    debugger
}

#[derive(Clone, Copy)]
struct LinkedRuntimePair {
    entry: DebuggerProgram,
    stack: DebuggerLinkedStackSnapshot,
    target: DebuggerLinkedScopeTarget,
    selector: DebuggerStaticScopeTarget,
    static_type: DebuggerStaticMetadataTypeId,
}

fn pause_and_pair_linked_runtime(
    debugger: &mut UnixStream,
    browser: &mut UnixStream,
    origin: &str,
) -> LinkedRuntimePair {
    hold_next_document(debugger, 1);
    navigate(browser, &format!("{origin}/"));
    let realm = one_realm(debugger_request(debugger, DebuggerRequest::ListPageRealms));
    let DebuggerReply::Programs(programs) =
        debugger_request(debugger, DebuggerRequest::ListPrograms { realm })
    else {
        panic!("linked runtime must expose two programs")
    };
    assert_eq!(programs.len(), 2);
    let (dependency, dependency_point) = programs
        .iter()
        .find_map(|program| {
            safe_points(
                debugger_request(
                    debugger,
                    DebuggerRequest::ListSafePoints { program: *program },
                ),
                *program,
            )
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .map(|point| (*program, point))
        })
        .expect("dependency function must have an entry safe point");
    let entry = *programs
        .iter()
        .find(|program| **program != dependency)
        .unwrap();
    let arm = DebuggerLinkedArmTarget {
        entry,
        dependency_safe_point: dependency_point,
    };
    assert_eq!(
        debugger_request(
            debugger,
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target: arm },
        ),
        DebuggerReply::LinkedNestedSafePointBreakpointArmed { target: arm }
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let stack = loop {
        match debugger_request(debugger, DebuggerRequest::GetLinkedExecutionState { entry }) {
            DebuggerReply::LinkedExecutionState {
                entry: observed,
                state,
            } if observed == entry => match *state {
                DebuggerLinkedExecutionState::Paused { stack } => break stack,
                DebuggerLinkedExecutionState::Pending if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                other => panic!("linked runtime did not pause: {other:?}"),
            },
            other => panic!("linked runtime returned {other:?}"),
        }
    };
    let (metadata, types, symbols) = static_receipts(debugger, entry);
    let scopes = linked_scopes(debugger, stack, 256);
    assert!(!scopes.scope_truncated);
    let target = scopes
        .entries
        .iter()
        .find_map(|scope_entry| {
            let target = DebuggerLinkedScopeTarget {
                stack,
                frame_index: 1,
                scope_entry: *scope_entry,
            };
            match debugger_request(debugger, DebuggerRequest::GetLinkedValue { target }) {
                DebuggerReply::LinkedValue(snapshot)
                    if snapshot.target == target
                        && snapshot.preview
                            == DebuggerValuePreview::NumberBits(9.0_f64.to_bits()) =>
                {
                    Some(target)
                }
                DebuggerReply::Error { .. } | DebuggerReply::LinkedValue(_) => None,
                other => panic!("linked Value returned {other:?}"),
            }
        })
        .expect("linked entry binding must hold 9");
    let selector = DebuggerStaticScopeTarget::Linked { metadata, target };
    let DebuggerReply::StaticScopeRelation(relation) = debugger_request(
        debugger,
        DebuggerRequest::GetStaticScopeRelation { target: selector },
    ) else {
        panic!("linked slot must have a static relation")
    };
    assert_eq!(relation.target, selector);
    assert!(types.contains(&relation.static_type));
    assert!(symbols.contains(&relation.symbol));
    let DebuggerReply::StaticMetadataType(display) = debugger_request(
        debugger,
        DebuggerRequest::DescribeStaticMetadataType {
            static_type: relation.static_type,
        },
    ) else {
        panic!("linked slot type must display")
    };
    assert_eq!(display.display, "number");
    LinkedRuntimePair {
        entry,
        stack,
        target,
        selector,
        static_type: relation.static_type,
    }
}

#[test]
fn launcher_pairs_linked_entry_type_and_value_with_independent_grants() {
    let gatekeeper_socket = clearing_gatekeeper();
    let mut fixture = LinkedRuntimeFixture::start();
    let mut launcher = LauncherProcess::spawn_with_owner_http_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            bounded_values: true,
            inventory: true,
            type_inventory: true,
            type_display: true,
            symbol_inventory: true,
            static_scope_relation: true,
            ..Default::default()
        },
        Some(&fixture.policy_file),
    );
    let mut browser = launcher.connect_browser();
    browser
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    // Negotiate before navigation so Hello cannot consume the pending arm window.
    let mut combined = timed_debugger(&launcher.debugger_socket, true, static_manifest());
    hold_next_document(&mut combined, 1);
    navigate(&mut browser, &format!("{}/", fixture.origin));
    let realm = one_realm(debugger_request(
        &mut combined,
        DebuggerRequest::ListPageRealms,
    ));
    let DebuggerReply::Programs(programs) =
        debugger_request(&mut combined, DebuggerRequest::ListPrograms { realm })
    else {
        panic!("the linked graph must expose two distinct programs")
    };
    assert_eq!(programs.len(), 2);
    let (dependency, dependency_point) = programs
        .iter()
        .find_map(|program| {
            safe_points(
                debugger_request(
                    &mut combined,
                    DebuggerRequest::ListSafePoints { program: *program },
                ),
                *program,
            )
            .into_iter()
            .find(|point| point.code_unit_ordinal == 1 && point.bytecode_offset == 0)
            .map(|point| (*program, point))
        })
        .expect("dependency function must expose its entry safe point");
    let entry = *programs
        .iter()
        .find(|program| **program != dependency)
        .unwrap();
    let arm = DebuggerLinkedArmTarget {
        entry,
        dependency_safe_point: dependency_point,
    };
    assert_eq!(
        debugger_request(
            &mut combined,
            DebuggerRequest::ArmLinkedNestedSafePointBreakpoint { target: arm },
        ),
        DebuggerReply::LinkedNestedSafePointBreakpointArmed { target: arm }
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let stack = loop {
        match debugger_request(
            &mut combined,
            DebuggerRequest::GetLinkedExecutionState { entry },
        ) {
            DebuggerReply::LinkedExecutionState {
                entry: observed,
                state,
            } if observed == entry => match *state {
                DebuggerLinkedExecutionState::Paused { stack } => break stack,
                DebuggerLinkedExecutionState::Pending if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                other => panic!("linked dependency did not pause: {other:?}"),
            },
            other => panic!("linked pause returned an invalid state: {other:?}"),
        }
    };
    assert_eq!(stack.frames[0].frame.program, dependency);
    assert_eq!(stack.frames[1].frame.program, entry);
    let (metadata, types, symbols) = static_receipts(&mut combined, entry);
    let scopes = linked_scopes(&mut combined, stack, 256);
    assert!(!scopes.scope_truncated);
    assert!(scopes.entries.len() > 1);
    let target = scopes
        .entries
        .iter()
        .find_map(|scope_entry| {
            let target = DebuggerLinkedScopeTarget {
                stack,
                frame_index: 1,
                scope_entry: *scope_entry,
            };
            match debugger_request(&mut combined, DebuggerRequest::GetLinkedValue { target }) {
                DebuggerReply::LinkedValue(snapshot)
                    if snapshot.target == target
                        && snapshot.preview
                            == DebuggerValuePreview::NumberBits(9.0_f64.to_bits()) =>
                {
                    Some(target)
                }
                DebuggerReply::Error { .. } | DebuggerReply::LinkedValue(_) => None,
                other => panic!("linked value read returned {other:?}"),
            }
        })
        .expect("initialized entry-root binding must contain 9");
    let selector = DebuggerStaticScopeTarget::Linked { metadata, target };
    let DebuggerReply::StaticScopeRelation(relation) = debugger_request(
        &mut combined,
        DebuggerRequest::GetStaticScopeRelation { target: selector },
    ) else {
        panic!("the same linked root slot must have a compiler type relation")
    };
    assert_eq!(relation.target, selector);
    assert!(types.contains(&relation.static_type));
    assert!(symbols.contains(&relation.symbol));
    let DebuggerReply::StaticMetadataType(display) = debugger_request(
        &mut combined,
        DebuggerRequest::DescribeStaticMetadataType {
            static_type: relation.static_type,
        },
    ) else {
        panic!("type display must use its separate grant and type receipt")
    };
    assert_eq!(display.static_type, relation.static_type);
    assert_eq!(display.display, "number");
    assert_eq!(
        debugger_request(&mut combined, DebuggerRequest::GetLinkedValue { target }),
        DebuggerReply::LinkedValue(Box::new(
            blueice_ipc::debugger::DebuggerLinkedValueSnapshot {
                target,
                preview: DebuggerValuePreview::NumberBits(9.0_f64.to_bits()),
            }
        ))
    );
    drop(combined);

    let mut static_only = timed_debugger(&launcher.debugger_socket, false, static_manifest());
    let (static_metadata, static_types, static_symbols) = static_receipts(&mut static_only, entry);
    assert!(!linked_scopes(&mut static_only, stack, 256).scope_truncated);
    let static_selector = DebuggerStaticScopeTarget::Linked {
        metadata: static_metadata,
        target,
    };
    let DebuggerReply::StaticScopeRelation(static_relation) = debugger_request(
        &mut static_only,
        DebuggerRequest::GetStaticScopeRelation {
            target: static_selector,
        },
    ) else {
        panic!("static-only stream must resolve the same compiler relation")
    };
    assert_eq!(static_relation.target, static_selector);
    assert!(static_types.contains(&static_relation.static_type));
    assert!(static_symbols.contains(&static_relation.symbol));
    assert!(matches!(
        debugger_request(&mut static_only, DebuggerRequest::GetLinkedValue { target },),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    drop(static_only);

    let relation_manifest =
        DebuggerMetadataCapabilityManifest::opaque_selected(DebuggerMetadataCapabilitySelection {
            static_scope_relation: true,
            ..Default::default()
        });
    let mut relation_only = timed_debugger(&launcher.debugger_socket, false, relation_manifest);
    let (relation_metadata, _, _) = static_receipts(&mut relation_only, entry);
    assert!(!linked_scopes(&mut relation_only, stack, 256).scope_truncated);
    let DebuggerReply::StaticScopeRelation(relation_without_display) = debugger_request(
        &mut relation_only,
        DebuggerRequest::GetStaticScopeRelation {
            target: DebuggerStaticScopeTarget::Linked {
                metadata: relation_metadata,
                target,
            },
        },
    ) else {
        panic!("relation-only stream must retain compiler IDs")
    };
    assert!(matches!(
        debugger_request(
            &mut relation_only,
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: relation_without_display.static_type,
            },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    drop(relation_only);

    let mut value_only = timed_debugger(
        &launcher.debugger_socket,
        true,
        DebuggerMetadataCapabilityManifest::empty(),
    );
    assert!(matches!(
        debugger_request(&mut value_only, DebuggerRequest::GetLinkedValue { target }),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    assert!(!linked_scopes(&mut value_only, stack, 256).scope_truncated);
    assert_eq!(
        debugger_request(&mut value_only, DebuggerRequest::GetLinkedValue { target }),
        DebuggerReply::LinkedValue(Box::new(
            blueice_ipc::debugger::DebuggerLinkedValueSnapshot {
                target,
                preview: DebuggerValuePreview::NumberBits(9.0_f64.to_bits()),
            }
        ))
    );
    assert!(matches!(
        debugger_request(
            &mut value_only,
            DebuggerRequest::GetStaticScopeRelation { target: selector },
        ),
        DebuggerReply::Error {
            code: DebuggerErrorCode::CapabilityUnavailable,
            ..
        }
    ));
    drop(value_only);
    let mut foreign = timed_debugger(
        &launcher.debugger_socket,
        true,
        DebuggerMetadataCapabilityManifest::empty(),
    );
    assert!(matches!(
        debugger_request(&mut foreign, DebuggerRequest::GetLinkedValue { target }),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    drop(foreign);
    let mut truncated = timed_debugger(
        &launcher.debugger_socket,
        true,
        DebuggerMetadataCapabilityManifest::empty(),
    );
    assert!(linked_scopes(&mut truncated, stack, 1).scope_truncated);
    assert!(matches!(
        debugger_request(&mut truncated, DebuggerRequest::GetLinkedValue { target }),
        DebuggerReply::Error {
            code: DebuggerErrorCode::InvalidTarget,
            ..
        }
    ));
    drop(truncated);

    launcher.shutdown();
    let requested = fixture.finish();
    assert!(requested.contains(&"/linked-entry.ts".to_string()));
    assert!(requested.contains(&"/linked-dependency.ts".to_string()));
    let _ = std::fs::remove_file(gatekeeper_socket);
}

fn assert_typed_refusal(
    debugger: &mut UnixStream,
    request: DebuggerRequest,
    allowed: &[DebuggerErrorCode],
) {
    let reply = debugger_request(debugger, request.clone());
    assert!(
        matches!(reply, DebuggerReply::Error { code, .. } if allowed.contains(&code)),
        "expired request {request:?} returned a partial or unexpected reply: {reply:?}"
    );
}

#[test]
fn launcher_expires_linked_type_value_pair_after_resume_and_http_reload() {
    let gatekeeper_socket = clearing_gatekeeper();
    let mut fixture = LinkedRuntimeFixture::start();
    let mut launcher = LauncherProcess::spawn_with_owner_http_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            bounded_values: true,
            inventory: true,
            type_inventory: true,
            type_display: true,
            symbol_inventory: true,
            static_scope_relation: true,
            ..Default::default()
        },
        Some(&fixture.policy_file),
    );
    let mut browser = launcher.connect_browser();
    browser
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let mut debugger = timed_debugger(&launcher.debugger_socket, true, static_manifest());
    let pair = pause_and_pair_linked_runtime(&mut debugger, &mut browser, &fixture.origin);

    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::ResumeLinkedNestedExecution {
                top_frame: pair.stack.frames[0].frame,
            },
        ),
        DebuggerReply::LinkedNestedResumeRequested {
            top_frame: pair.stack.frames[0].frame,
        }
    );
    for request in [
        DebuggerRequest::GetStaticScopeRelation {
            target: pair.selector,
        },
        DebuggerRequest::GetLinkedValue {
            target: pair.target,
        },
    ] {
        assert_typed_refusal(
            &mut debugger,
            request,
            &[
                DebuggerErrorCode::InvalidTarget,
                DebuggerErrorCode::InvalidExecutionState,
            ],
        );
    }
    // The compiler display is generation-bound metadata and remains valid
    // while that program exists; it is not itself a paused slot snapshot.
    assert!(matches!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeStaticMetadataType {
                static_type: pair.static_type,
            },
        ),
        DebuggerReply::StaticMetadataType(_)
    ));

    navigate(&mut browser, &format!("{}/", fixture.origin));
    for request in [
        DebuggerRequest::GetStaticScopeRelation {
            target: pair.selector,
        },
        DebuggerRequest::DescribeStaticMetadataType {
            static_type: pair.static_type,
        },
        DebuggerRequest::GetLinkedValue {
            target: pair.target,
        },
    ] {
        assert_typed_refusal(
            &mut debugger,
            request,
            &[
                DebuggerErrorCode::StaleRealm,
                DebuggerErrorCode::StaleProgram,
                DebuggerErrorCode::InvalidTarget,
                DebuggerErrorCode::InvalidExecutionState,
                DebuggerErrorCode::CapabilityUnavailable,
            ],
        );
    }
    drop(debugger);
    launcher.shutdown();
    let requested = fixture.finish();
    assert!(requested.iter().filter(|path| path.as_str() == "/").count() >= 2);
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_rejects_predecessor_linked_type_value_pair_after_child_cutover() {
    let gatekeeper_socket = clearing_gatekeeper();
    let mut fixture = LinkedRuntimeFixture::start();
    let mut launcher = LauncherProcess::spawn_with_owner_http_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            bounded_values: true,
            inventory: true,
            type_inventory: true,
            type_display: true,
            symbol_inventory: true,
            static_scope_relation: true,
            ..Default::default()
        },
        Some(&fixture.policy_file),
    );
    let mut browser = launcher.connect_browser();
    browser
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    let mut debugger = timed_debugger(&launcher.debugger_socket, true, static_manifest());
    // Match the successor's reminted realm ordinal to exercise the old
    // core-instance guard even when other visible identifiers coincide.
    navigate(&mut browser, &format!("{}/", fixture.origin));
    let predecessor = pause_and_pair_linked_runtime(&mut debugger, &mut browser, &fixture.origin);

    let mut control = UnixStream::connect(&launcher.control_socket).unwrap();
    write_control_request(&mut control, &ControlRequest::Cutover).unwrap();
    assert_eq!(
        read_control_reply(&mut control).unwrap(),
        ControlReply::CutoverDone { tabs_migrated: 1 }
    );
    drop(debugger);
    drop(browser);
    let mut successor_browser = launcher.connect_browser();
    successor_browser
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    blueice_ipc::client_handshake(&mut successor_browser).unwrap();
    let mut successor_debugger = timed_debugger(&launcher.debugger_socket, true, static_manifest());
    let successor = pause_and_pair_linked_runtime(
        &mut successor_debugger,
        &mut successor_browser,
        &fixture.origin,
    );
    assert_eq!(successor.entry, predecessor.entry);
    assert_ne!(
        successor.stack.frames[0].frame.core_instance,
        predecessor.stack.frames[0].frame.core_instance
    );
    assert_ne!(
        successor.static_type.metadata,
        predecessor.static_type.metadata
    );
    assert_eq!(
        debugger_request(
            &mut successor_debugger,
            DebuggerRequest::GetLinkedValue {
                target: successor.target,
            },
        ),
        DebuggerReply::LinkedValue(Box::new(
            blueice_ipc::debugger::DebuggerLinkedValueSnapshot {
                target: successor.target,
                preview: DebuggerValuePreview::NumberBits(9.0_f64.to_bits()),
            }
        ))
    );
    for request in [
        DebuggerRequest::GetStaticScopeRelation {
            target: predecessor.selector,
        },
        DebuggerRequest::DescribeStaticMetadataType {
            static_type: predecessor.static_type,
        },
        DebuggerRequest::GetLinkedValue {
            target: predecessor.target,
        },
    ] {
        assert_typed_refusal(
            &mut successor_debugger,
            request,
            &[
                DebuggerErrorCode::StaleRealm,
                DebuggerErrorCode::StaleProgram,
                DebuggerErrorCode::InvalidTarget,
                DebuggerErrorCode::InvalidExecutionState,
                DebuggerErrorCode::CapabilityUnavailable,
            ],
        );
    }
    drop(successor_debugger);
    launcher.shutdown();
    let requested = fixture.finish();
    assert!(requested.iter().filter(|path| path.as_str() == "/").count() >= 2);
    let _ = std::fs::remove_file(gatekeeper_socket);
}
