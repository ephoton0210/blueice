// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn launcher_reports_original_classic_and_module_nested_exception_positions() {
    for (mime, source, slug) in [
        (
            "application/x-blueice-typescript",
            CLASSIC_THROW_BLUETS_SOURCE,
            "classic",
        ),
        (
            "application/x-blueice-typescript-module",
            MODULE_THROW_BLUETS_SOURCE,
            "module",
        ),
    ] {
        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<main>{slug}-exception-location</main><script type=\"{mime}\">{source}</script>"
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        });
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                inventory: true,
                source_inventory: true,
                safe_point_span: true,
                ..StaticMetadataPolicy::default()
            },
        );
        let mut browser = launcher.connect_browser();
        blueice_ipc::client_handshake(&mut browser).unwrap();
        navigate(&mut browser, &url);
        fixture.join().unwrap();

        let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
        let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::Hello {
                    protocol_version: DEBUGGER_PROTOCOL_VERSION,
                    requested_bounded_values: false,
                    requested_metadata_capabilities: manifest.clone(),
                },
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_bounded_values: false,
                granted_metadata_capabilities: manifest,
            }
        );
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let program = one_program(
            debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
            realm,
        );
        let DebuggerReply::StaticMetadata(metadata) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata { program },
        ) else {
            panic!("typed {slug} program must retain one metadata handle")
        };
        let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataSources {
                metadata: metadata[0],
            },
        ) else {
            panic!("typed {slug} program must retain source IDs")
        };
        assert!(!sources.is_empty());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            thread::sleep(Duration::from_millis(20));
            match debugger_request(
                &mut debugger,
                DebuggerRequest::GetExecutionState { program },
            ) {
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Completed,
                    ..
                } => break,
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Pending,
                    ..
                } if Instant::now() < deadline => {}
                reply => panic!("typed {slug} throw did not complete: {reply:?}"),
            }
        }
        let mut locations = Vec::new();
        for source_id in sources.iter().copied() {
            match debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeExceptionLocation { source: source_id },
            ) {
                DebuggerReply::ExceptionLocation(location) => locations.push(location),
                DebuggerReply::Error {
                    code: DebuggerErrorCode::InvalidTarget,
                    ..
                } => {}
                reply => panic!("typed {slug} throw must return a location or refusal: {reply:?}"),
            }
        }
        assert_eq!(
            locations.len(),
            1,
            "{slug} must have exactly one originating source"
        );
        let location = locations[0];
        assert!(sources.contains(&location.source));
        assert_eq!(location.safe_point.program, program);
        assert_eq!(location.safe_point.code_unit_ordinal, 1);
        assert!(safe_points(
            debugger_request(&mut debugger, DebuggerRequest::ListSafePoints { program }),
            program,
        )
        .contains(&location.safe_point));
        assert!(location.is_well_formed());
        let function_start = source.find("function fail").unwrap();
        let function_end = source.find("} ").unwrap() + 1;
        assert_eq!(location.start_byte as usize, function_start);
        assert_eq!(location.end_byte as usize, function_end);
        assert_eq!(location.coordinates.start_line, 0);
        assert_eq!(location.coordinates.end_line, 0);
        assert_eq!(
            location.coordinates.start_column_utf16 as usize,
            source[..function_start].encode_utf16().count()
        );
        assert_eq!(
            location.coordinates.end_column_utf16 as usize,
            source[..function_end].encode_utf16().count()
        );
        assert_ne!(
            location.start_byte as usize, location.coordinates.start_column_utf16 as usize,
            "the astral prefix must distinguish UTF-8 bytes from UTF-16 columns"
        );
        assert!(!format!("{location:?}").contains("throw"));
        drop(debugger);
        launcher.shutdown();
        let _ = std::fs::remove_file(gatekeeper_socket);
    }
}

#[test]
fn launcher_refuses_exception_locations_without_owner_client_grants_or_source_receipts() {
    for (owner_span, client_span) in [(false, true), (true, false), (true, true)] {
        let gatekeeper_socket = clearing_gatekeeper();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let fixture = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let body = format!(
                "<script type=\"application/x-blueice-typescript\">{CLASSIC_THROW_BLUETS_SOURCE}</script>"
            );
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        });
        let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
            &gatekeeper_socket,
            StaticMetadataPolicy {
                inventory: true,
                source_inventory: true,
                safe_point_span: owner_span,
                ..StaticMetadataPolicy::default()
            },
        );
        let mut browser = launcher.connect_browser();
        blueice_ipc::client_handshake(&mut browser).unwrap();
        navigate(&mut browser, &url);
        fixture.join().unwrap();

        let requested = if client_span {
            DebuggerMetadataCapabilityManifest::opaque_safe_point_span()
        } else {
            DebuggerMetadataCapabilityManifest::opaque_source_inventory()
        };
        let granted = if owner_span && client_span {
            DebuggerMetadataCapabilityManifest::opaque_safe_point_span()
        } else {
            DebuggerMetadataCapabilityManifest::opaque_source_inventory()
        };
        let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
        assert_eq!(
            debugger_request(
                &mut debugger,
                DebuggerRequest::Hello {
                    protocol_version: DEBUGGER_PROTOCOL_VERSION,
                    requested_bounded_values: false,
                    requested_metadata_capabilities: requested,
                },
            ),
            DebuggerReply::HelloAck {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                granted_bounded_values: false,
                granted_metadata_capabilities: granted,
            }
        );
        let realm = one_realm(debugger_request(
            &mut debugger,
            DebuggerRequest::ListPageRealms,
        ));
        let program = one_program(
            debugger_request(&mut debugger, DebuggerRequest::ListPrograms { realm }),
            realm,
        );
        let DebuggerReply::StaticMetadata(metadata) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadata { program },
        ) else {
            panic!("the BlueTS program must expose a metadata handle")
        };
        assert_eq!(metadata.len(), 1);
        let DebuggerReply::StaticMetadataSources(sources) = debugger_request(
            &mut debugger,
            DebuggerRequest::ListStaticMetadataSources {
                metadata: metadata[0],
            },
        ) else {
            panic!("the BlueTS program must expose source IDs")
        };
        assert!(!sources.is_empty());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match debugger_request(
                &mut debugger,
                DebuggerRequest::GetExecutionState { program },
            ) {
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Completed,
                    ..
                } => break,
                DebuggerReply::ExecutionState {
                    state: DebuggerExecutionState::Pending,
                    ..
                } if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                reply => panic!("throw did not complete: {reply:?}"),
            }
        }
        if !owner_span || !client_span {
            let source = sources[0];
            assert_exception_refusal(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::DescribeExceptionLocation { source },
                ),
                DebuggerErrorCode::CapabilityUnavailable,
            );
            drop(debugger);
        } else {
            let valid_sources = sources
                .iter()
                .copied()
                .filter(|source| {
                    matches!(
                        debugger_request(
                            &mut debugger,
                            DebuggerRequest::DescribeExceptionLocation { source: *source },
                        ),
                        DebuggerReply::ExceptionLocation(_)
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(valid_sources.len(), 1);
            let source = valid_sources[0];
            drop(debugger);
            let mut separate = UnixStream::connect(&launcher.debugger_socket).unwrap();
            let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
            assert_eq!(
                debugger_request(
                    &mut separate,
                    DebuggerRequest::Hello {
                        protocol_version: DEBUGGER_PROTOCOL_VERSION,
                        requested_bounded_values: false,
                        requested_metadata_capabilities: manifest.clone(),
                    },
                ),
                DebuggerReply::HelloAck {
                    protocol_version: DEBUGGER_PROTOCOL_VERSION,
                    granted_bounded_values: false,
                    granted_metadata_capabilities: manifest,
                }
            );
            assert_exception_refusal(
                debugger_request(
                    &mut separate,
                    DebuggerRequest::DescribeExceptionLocation { source },
                ),
                DebuggerErrorCode::InvalidTarget,
            );
            assert_eq!(
                debugger_request(
                    &mut separate,
                    DebuggerRequest::ListStaticMetadata { program },
                ),
                DebuggerReply::StaticMetadata(metadata.clone())
            );
            assert_exception_refusal(
                debugger_request(
                    &mut separate,
                    DebuggerRequest::DescribeExceptionLocation { source },
                ),
                DebuggerErrorCode::InvalidTarget,
            );
            assert_eq!(
                debugger_request(
                    &mut separate,
                    DebuggerRequest::ListStaticMetadataSources {
                        metadata: metadata[0],
                    },
                ),
                DebuggerReply::StaticMetadataSources(sources)
            );
            assert!(matches!(
                debugger_request(
                    &mut separate,
                    DebuggerRequest::DescribeExceptionLocation { source },
                ),
                DebuggerReply::ExceptionLocation(_)
            ));
        }
        launcher.shutdown();
        let _ = std::fs::remove_file(gatekeeper_socket);
    }
}

#[test]
fn launcher_refuses_normal_and_stale_exception_locations() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        for body in [
            format!(
                "<script type=\"application/x-blueice-typescript\">{CLASSIC_THROW_BLUETS_SOURCE}</script>"
            ),
            "<script type=\"application/x-blueice-typescript\">const answer: number = 3;</script>"
                .to_string(),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        }
    });
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &format!("{base_url}/throw"));

    let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: manifest,
        }
    );
    let (throw_program, throw_sources) = one_receipted_typed_program(&mut debugger, "throw");
    await_terminal_bluets_execution(&mut debugger, throw_program);
    let throw_locations = throw_sources
        .iter()
        .copied()
        .filter(|source| {
            matches!(
                debugger_request(
                    &mut debugger,
                    DebuggerRequest::DescribeExceptionLocation { source: *source },
                ),
                DebuggerReply::ExceptionLocation(_)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(throw_locations.len(), 1);
    let old_source = throw_locations[0];

    navigate(&mut browser, &format!("{base_url}/normal"));
    assert_exception_refusal(
        debugger_request(
            &mut debugger,
            DebuggerRequest::DescribeExceptionLocation { source: old_source },
        ),
        DebuggerErrorCode::StaleRealm,
    );
    let (normal_program, normal_sources) = one_receipted_typed_program(&mut debugger, "normal");
    assert_ne!(normal_program, throw_program);
    await_terminal_bluets_execution(&mut debugger, normal_program);
    for source in normal_sources {
        assert_exception_refusal(
            debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeExceptionLocation { source },
            ),
            DebuggerErrorCode::InvalidExecutionState,
        );
    }

    fixture.join().unwrap();
    drop(debugger);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}

#[test]
fn launcher_refuses_caught_bluets_exception_locations() {
    let gatekeeper_socket = clearing_gatekeeper();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let fixture = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        let body = format!(
            "<script type=\"application/x-blueice-typescript\">{CAUGHT_THROW_BLUETS_SOURCE}</script>"
        );
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    let mut launcher = LauncherProcess::spawn_with_static_metadata_policy(
        &gatekeeper_socket,
        StaticMetadataPolicy {
            inventory: true,
            source_inventory: true,
            safe_point_span: true,
            ..StaticMetadataPolicy::default()
        },
    );
    let mut browser = launcher.connect_browser();
    blueice_ipc::client_handshake(&mut browser).unwrap();
    navigate(&mut browser, &url);
    fixture.join().unwrap();
    let manifest = DebuggerMetadataCapabilityManifest::opaque_safe_point_span();
    let mut debugger = UnixStream::connect(&launcher.debugger_socket).unwrap();
    assert_eq!(
        debugger_request(
            &mut debugger,
            DebuggerRequest::Hello {
                protocol_version: DEBUGGER_PROTOCOL_VERSION,
                requested_bounded_values: false,
                requested_metadata_capabilities: manifest.clone(),
            },
        ),
        DebuggerReply::HelloAck {
            protocol_version: DEBUGGER_PROTOCOL_VERSION,
            granted_bounded_values: false,
            granted_metadata_capabilities: manifest,
        }
    );
    let (program, sources) = one_receipted_typed_program(&mut debugger, "caught");
    await_terminal_bluets_execution(&mut debugger, program);
    for source in sources {
        assert_exception_refusal(
            debugger_request(
                &mut debugger,
                DebuggerRequest::DescribeExceptionLocation { source },
            ),
            DebuggerErrorCode::InvalidExecutionState,
        );
    }
    drop(debugger);
    launcher.shutdown();
    let _ = std::fs::remove_file(gatekeeper_socket);
}
