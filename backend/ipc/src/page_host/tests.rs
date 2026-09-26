// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use std::os::unix::net::UnixStream;

mod debugger_shapes;

fn source() -> PageHostSource {
    PageHostSource::new("blueice://page/main.js", "globalThis.answer = 42;")
}

fn document() -> PageHostDocument {
    PageHostDocument {
        tab_id: 7,
        document_generation: 3,
        snapshot: PageHostDocumentSnapshot {
            document_text: "snapshot text".to_string(),
            document_origin: "https://example.test".to_string(),
        },
        debugger_execution_control: true,
        scripts: vec![PageHostScript {
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Classic,
            graph: PageHostModuleGraph {
                entry: "blueice://page/main.js".to_string(),
                modules: vec![source()],
                resolutions: vec![],
                resolver_fingerprint: "core-loader-v1".to_string(),
            },
        }],
    }
}

#[test]
fn requests_and_replies_round_trip_over_a_real_socket() {
    let requests = [
        PageHostRequest::Hello {
            protocol_version: PAGE_HOST_PROTOCOL_VERSION,
            session_token: "not-a-real-token".to_string(),
        },
        PageHostRequest::SynchronizeDocument {
            document: document(),
        },
        PageHostRequest::DispatchClick {
            tab_id: 7,
            document_generation: 3,
            node_id: 42,
        },
        PageHostRequest::CloseRealm {
            tab_id: 7,
            document_generation: 3,
        },
        PageHostRequest::GetRealmStats {
            tab_id: 7,
            document_generation: 3,
        },
        PageHostRequest::GetChildStats,
        PageHostRequest::ListDebuggerPrograms {
            tab_id: 7,
            document_generation: 3,
        },
        PageHostRequest::ListDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
        },
        PageHostRequest::DescribeDebuggerBlueTsMetadata {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
        },
        PageHostRequest::DescribeDebuggerBlueTsMetadataLoweringSummary {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
        },
        PageHostRequest::ListDebuggerBlueTsMetadataSources {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
        },
        PageHostRequest::ListDebuggerBlueTsMetadataTypes {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
        },
        PageHostRequest::DescribeDebuggerBlueTsMetadataType {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            type_id: 0,
        },
        PageHostRequest::ListDebuggerBlueTsMetadataSymbols {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
        },
        PageHostRequest::ListDebuggerBlueTsMetadataContracts {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
        },
        PageHostRequest::DescribeDebuggerBlueTsMetadataContract {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            contract_id: 0,
        },
        PageHostRequest::ValidateDebuggerBlueTsMetadataContract {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            contract_id: 0,
            value: CompilerContractValue::Object(
                [("enabled".to_string(), CompilerContractValue::Boolean(true))]
                    .into_iter()
                    .collect(),
            ),
        },
        PageHostRequest::DescribeDebuggerBlueTsMetadataSymbol {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            symbol_id: 0,
        },
        PageHostRequest::DescribeDebuggerBlueTsMetadataSymbolLocation {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            symbol_id: 0,
        },
        PageHostRequest::DescribeDebuggerBlueTsSafePointSpan {
            tab_id: 7,
            document_generation: 3,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
        },
        PageHostRequest::DescribeDebuggerBlueTsExceptionLocation {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
        },
        PageHostRequest::ResolveDebuggerBlueTsSourceBreakpoint {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            source_id: 0,
            source_byte: 4,
        },
        PageHostRequest::ListDebuggerSafePoints {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
        },
        PageHostRequest::ValidateDebuggerSafePoint {
            tab_id: 7,
            document_generation: 3,
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
        },
        PageHostRequest::SetDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 3,
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
        },
        PageHostRequest::ListDebuggerBreakpoints {
            tab_id: 7,
            document_generation: 3,
        },
        PageHostRequest::ClearDebuggerBreakpoint {
            tab_id: 7,
            document_generation: 3,
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
        },
        PageHostRequest::ArmDebuggerRootSafePointBreakpoint {
            tab_id: 7,
            document_generation: 3,
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
        },
        PageHostRequest::GetDebuggerExecutionState {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
        },
        PageHostRequest::ResumeDebuggerExecution {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
        },
        PageHostRequest::StepDebuggerRootInstruction {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
        },
        PageHostRequest::StepDebuggerBlueTsSourceSpan {
            tab_id: 7,
            document_generation: 3,
            metadata: PageHostDebuggerMetadataHandle {
                metadata_handle: 17,
                metadata_generation: 19,
            },
            source_id: 0,
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
        },
        PageHostRequest::AdvanceDebuggerExecution {
            tab_id: 7,
            document_generation: 3,
        },
        PageHostRequest::Shutdown,
        PageHostRequest::Unknown,
    ];
    for request in requests {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_request(&mut writer, &request).unwrap();
        assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
    }

    let reply = PageHostReply::Synchronized {
        tab_id: 7,
        document_generation: 3,
        already_current: false,
        reports: vec![PageHostScriptReport {
            tab_id: 7,
            document_generation: 3,
            ordinal: 0,
            language: PageHostScriptLanguage::JavaScript,
            kind: PageHostScriptKind::Classic,
            outcome: PageHostScriptOutcome::Executed,
        }],
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);

    let debugger_reply = PageHostReply::DebuggerBreakpointCleared {
        tab_id: 7,
        document_generation: 3,
        safe_point: PageHostDebuggerSafePoint {
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        },
        was_present: true,
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsSourceStepRequested {
        tab_id: 7,
        document_generation: 3,
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        source_id: 0,
        safe_point: PageHostDebuggerSafePoint {
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsSafePointSpan {
        tab_id: 7,
        document_generation: 3,
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        safe_point: PageHostDebuggerSafePoint {
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        },
        span: PageHostDebuggerBlueTsSafePointSpan {
            source_id: 0,
            start_byte: 0,
            end_byte: 25,
            coordinates: DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 0,
                end_line: 0,
                end_column_utf16: 25,
            },
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsExceptionLocation {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        location: PageHostDebuggerBlueTsExceptionLocation {
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 1,
                bytecode_offset: 4,
            },
            span: PageHostDebuggerBlueTsSafePointSpan {
                source_id: 0,
                start_byte: 2,
                end_byte: 25,
                coordinates: DebuggerSourceCoordinates {
                    start_line: 0,
                    start_column_utf16: 2,
                    end_line: 0,
                    end_column_utf16: 25,
                },
            },
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsSourceBreakpoint {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        source_id: 0,
        source_byte: 4,
        safe_point: Some(PageHostDebuggerSafePoint {
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            code_unit_ordinal: 0,
            bytecode_offset: 4,
        }),
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsSourceBreakpoint {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        source_id: 0,
        source_byte: 30,
        safe_point: None,
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadata {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: vec![PageHostDebuggerMetadataHandle {
            metadata_handle: 1 << 63,
            metadata_generation: 1 << 63,
        }],
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSummary {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        summary: PageHostDebuggerBlueTsMetadataSummary {
            language_version: "blue-ts-0.1".to_string(),
            compiler_options_hash: "0123456789abcdef".to_string(),
            source_count: 1,
            type_count: 2,
            symbol_count: 3,
            contract_count: 4,
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataLoweringSummary {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        summary: Box::new(PageHostDebuggerBlueTsMetadataLoweringSummary {
            safe_point_map_abi: "bluejs-safe-point-map-v1".to_string(),
            program_abi: "bluejs-program-v1".to_string(),
            source_set_hash: "bts-source-set-0123456789abcdef".to_string(),
            bound_safe_point_count: 1,
        }),
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSources {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        sources: vec![PageHostDebuggerBlueTsMetadataSourceId { source_id: 0 }],
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataTypes {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        types: vec![PageHostDebuggerBlueTsMetadataTypeId { type_id: 0 }],
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSymbols {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        symbols: vec![PageHostDebuggerBlueTsMetadataSymbolId { symbol_id: 0 }],
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSymbol {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        symbol: PageHostDebuggerBlueTsMetadataSymbolDisplay {
            symbol_id: 0,
            display: "ProjectControlledName".to_string(),
            kind: DebuggerStaticMetadataSymbolKind::Interface,
            exported: true,
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataSymbolLocation {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        location: PageHostDebuggerBlueTsMetadataSymbolLocation {
            symbol_id: 0,
            source_id: 0,
            start_byte: 6,
            end_byte: 31,
            coordinates: DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 6,
                end_line: 0,
                end_column_utf16: 31,
            },
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataContract {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        contract: PageHostDebuggerBlueTsMetadataContractDisplay {
            contract_id: 0,
            display: "ProjectControlledContract".to_string(),
            root_kind: DebuggerStaticMetadataContractRootKind::Record,
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataContractValidation {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        validation: PageHostDebuggerBlueTsMetadataContractValidation {
            contract_id: 0,
            valid: true,
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerBlueTsMetadataContracts {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        metadata: PageHostDebuggerMetadataHandle {
            metadata_handle: 17,
            metadata_generation: 19,
        },
        contracts: vec![PageHostDebuggerBlueTsMetadataContractId { contract_id: 0 }],
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);

    let debugger_reply = PageHostReply::DebuggerExecutionState {
        tab_id: 7,
        document_generation: 3,
        program: PageHostDebuggerProgram {
            program_handle: 11,
            program_generation: 13,
        },
        state: PageHostDebuggerExecutionState::Paused {
            safe_point: PageHostDebuggerSafePoint {
                program: PageHostDebuggerProgram {
                    program_handle: 11,
                    program_generation: 13,
                },
                code_unit_ordinal: 0,
                bytecode_offset: 4,
            },
        },
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &debugger_reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);
    for debugger_reply in [
        PageHostReply::DebuggerExecutionState {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
            state: PageHostDebuggerExecutionState::Stepping,
        },
        PageHostReply::DebuggerExecutionStepRequested {
            tab_id: 7,
            document_generation: 3,
            program: PageHostDebuggerProgram {
                program_handle: 11,
                program_generation: 13,
            },
        },
    ] {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        write_page_host_reply(&mut writer, &debugger_reply).unwrap();
        assert_eq!(read_page_host_reply(&mut reader).unwrap(), debugger_reply);
    }
}

#[test]
fn handshake_requires_the_exact_version_and_capability() {
    let token = "launcher-secret";
    assert_eq!(
        negotiate(
            &PageHostRequest::Hello {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                session_token: token.to_string(),
            },
            token,
        ),
        PageHostReply::HelloAck {
            protocol_version: PAGE_HOST_PROTOCOL_VERSION,
        }
    );
    assert!(matches!(
        negotiate(
            &PageHostRequest::Hello {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION - 1,
                session_token: token.to_string(),
            },
            token,
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::ProtocolVersion,
            ..
        }
    ));
    assert!(matches!(
        negotiate(
            &PageHostRequest::Hello {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION + 1,
                session_token: token.to_string(),
            },
            token,
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::ProtocolVersion,
            ..
        }
    ));
    assert!(matches!(
        negotiate(
            &PageHostRequest::Hello {
                protocol_version: PAGE_HOST_PROTOCOL_VERSION,
                session_token: "wrong".to_string(),
            },
            token,
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::Authentication,
            ..
        }
    ));
    assert!(matches!(
        negotiate(
            &PageHostRequest::SynchronizeDocument {
                document: document(),
            },
            token,
        ),
        PageHostReply::Error {
            code: PageHostErrorCode::ProtocolVersion,
            ..
        }
    ));
}

#[test]
fn contract_location_round_trips_without_a_plan_or_source_record() {
    let program = PageHostDebuggerProgram {
        program_handle: 11,
        program_generation: 13,
    };
    let metadata = PageHostDebuggerMetadataHandle {
        metadata_handle: 17,
        metadata_generation: 19,
    };
    let request = PageHostRequest::DescribeDebuggerBlueTsMetadataContractLocation {
        tab_id: 7,
        document_generation: 3,
        program,
        metadata,
        contract_id: 0,
    };
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_request(&mut writer, &request).unwrap();
    assert_eq!(read_page_host_request(&mut reader).unwrap(), request);
    let reply = PageHostReply::DebuggerBlueTsMetadataContractLocation {
        tab_id: 7,
        document_generation: 3,
        program,
        metadata,
        location: PageHostDebuggerBlueTsMetadataContractLocation {
            contract_id: 0,
            source_id: 0,
            start_byte: 6,
            end_byte: 31,
            coordinates: DebuggerSourceCoordinates {
                start_line: 0,
                start_column_utf16: 6,
                end_line: 0,
                end_column_utf16: 31,
            },
        },
    };
    assert!(!format!("{reply:?}").contains("PrivateContract"));
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
}

#[test]
fn source_constructor_fingerprints_exact_bytes() {
    let source = PageHostSource::new("blueice://page/main.js", "let answer = 42;");
    assert_eq!(source.source_hash, source_hash(&source.source));
    assert_ne!(source.source_hash, source_hash("let answer = 43;"));
}

#[test]
fn realm_stats_reject_conversion_sentinels_and_program_overflow() {
    let stats = PageHostRealmStats {
        tab_id: 7,
        document_generation: 3,
        program_count: 2,
        bytecode_bytes: 64,
        heap_bytes: 128,
    };
    assert!(stats.is_well_formed());
    assert!(!PageHostRealmStats {
        tab_id: 0,
        ..stats.clone()
    }
    .is_well_formed());
    assert!(!PageHostRealmStats {
        program_count: PAGE_HOST_REALM_STATS_MAX_PROGRAMS + 1,
        ..stats.clone()
    }
    .is_well_formed());
    assert!(!PageHostRealmStats {
        bytecode_bytes: u64::MAX,
        ..stats.clone()
    }
    .is_well_formed());
    assert!(!PageHostRealmStats {
        heap_bytes: u64::MAX,
        ..stats
    }
    .is_well_formed());
}

#[test]
fn child_stats_round_trip_and_reject_impossible_totals() {
    let stats = PageHostChildStats {
        realm_count: 2,
        program_count: 3,
        bytecode_bytes: 64,
        heap_bytes: 128,
    };
    assert!(stats.is_well_formed());
    for invalid in [
        PageHostChildStats {
            realm_count: 0,
            ..stats
        },
        PageHostChildStats {
            realm_count: u32::MAX,
            ..stats
        },
        PageHostChildStats {
            program_count: u64::from(PAGE_HOST_REALM_STATS_MAX_PROGRAMS) * 2 + 1,
            ..stats
        },
        PageHostChildStats {
            bytecode_bytes: u64::MAX,
            ..stats
        },
        PageHostChildStats {
            program_count: 0,
            ..stats
        },
        PageHostChildStats {
            heap_bytes: u64::MAX,
            ..stats
        },
    ] {
        assert!(!invalid.is_well_formed());
    }
    let reply = PageHostReply::ChildStats(stats);
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    write_page_host_reply(&mut writer, &reply).unwrap();
    assert_eq!(read_page_host_reply(&mut reader).unwrap(), reply);
}

#[test]
fn page_host_rejects_an_oversized_frame_before_payload_allocation() {
    let oversized = u32::try_from(PAGE_HOST_MAX_FRAME_BYTES + 1).unwrap();
    let mut bytes = oversized.to_le_bytes().to_vec();
    assert!(read_page_host_request(&mut std::io::Cursor::new(&mut bytes)).is_err());
}

#[test]
fn version_five_document_requires_the_fixed_core_snapshot() {
    let mut value = serde_json::to_value(PageHostRequest::SynchronizeDocument {
        document: document(),
    })
    .unwrap();
    value["SynchronizeDocument"]["document"]
        .as_object_mut()
        .unwrap()
        .remove("snapshot");
    assert!(serde_json::from_value::<PageHostRequest>(value).is_err());
}
