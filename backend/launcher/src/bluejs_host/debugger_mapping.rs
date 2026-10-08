// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// The only BlueTS throw-site mapping: one exact installed instruction in the
/// compiler-verified attachment, never a nearest source or generated offset.
pub(super) fn exact_bluets_span_for_site(
    retained: &RetainedDirectDebugInfo,
    safe_point: PageHostDebuggerSafePoint,
) -> Option<PageHostDebuggerBlueTsSafePointSpan> {
    let entry = retained
        .safe_point_map()
        .source_span_for_safe_point(safe_point.code_unit_ordinal, safe_point.bytecode_offset)?;
    let mut matching_sources = retained
        .static_info()
        .sources
        .iter()
        .filter(|source| source.module == entry.source);
    let source = matching_sources.next()?;
    if matching_sources.next().is_some()
        || entry.start_byte >= entry.end_byte
        || entry.end_byte > usize::try_from(DEBUGGER_STATIC_METADATA_MAX_SOURCE_SPAN_BYTES).ok()?
    {
        return None;
    }
    let start_byte = u32::try_from(entry.start_byte).ok()?;
    let end_byte = u32::try_from(entry.end_byte).ok()?;
    let coordinates = debugger_source_coordinates(entry.location, start_byte, end_byte)?;
    Some(PageHostDebuggerBlueTsSafePointSpan {
        source_id: source.id.0,
        start_byte,
        end_byte,
        coordinates,
    })
}

pub(super) fn debugger_source_coordinates(
    location: blueice_bluets::DebugSourceLocation,
    start_byte: u32,
    end_byte: u32,
) -> Option<DebuggerSourceCoordinates> {
    let coordinates = DebuggerSourceCoordinates {
        start_line: u32::try_from(location.start.line).ok()?,
        start_column_utf16: u32::try_from(location.start.column_utf16).ok()?,
        end_line: u32::try_from(location.end.line).ok()?,
        end_column_utf16: u32::try_from(location.end.column_utf16).ok()?,
    };
    coordinates
        .is_well_formed_for_range(start_byte, end_byte)
        .then_some(coordinates)
}

pub(super) fn bluets_bridge_category(error: BridgeError) -> &'static str {
    match error {
        BridgeError::BlueTs(_) => "BlueTS compilation rejected the page script",
        BridgeError::PageRuntime(error) => page_runtime_category(error),
        BridgeError::BlueJs(_) | BridgeError::BlueJsDebug(_) => {
            "BlueJS compilation rejected the direct BlueTS page script"
        }
        BridgeError::UnsupportedRuntimeTarget { .. }
        | BridgeError::InvalidSourceIdentity(_)
        | BridgeError::ProvenanceAttachment(_)
        | BridgeError::DebugAttachment(_) => "BlueTS direct lowering rejected the page script",
    }
}

pub(super) fn page_runtime_category(error: BlueJsPageRuntimeError) -> &'static str {
    match error {
        BlueJsPageRuntimeError::BytecodeLimit { .. }
        | BlueJsPageRuntimeError::ProgramLimit { .. }
        | BlueJsPageRuntimeError::RealmLimit { .. } => {
            "JavaScript page resource policy rejected the page script"
        }
        BlueJsPageRuntimeError::Runtime(RuntimeError::ModuleResolution(_)) => {
            "authorized JavaScript graph rejected the page script"
        }
        BlueJsPageRuntimeError::Runtime(_) => "BlueJS page execution failed",
        _ => "BlueJS page host rejected the page script",
    }
}

pub(super) fn rejected(category: &'static str) -> PageHostScriptOutcome {
    PageHostScriptOutcome::Rejected {
        category: category.to_string(),
    }
}

pub(super) fn script_report(
    tab_id: u64,
    document_generation: u64,
    ordinal: u32,
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    outcome: PageHostScriptOutcome,
) -> PageHostScriptReport {
    script_report_with_position(
        tab_id,
        document_generation,
        ordinal,
        language,
        kind,
        outcome,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn script_report_with_position(
    tab_id: u64,
    document_generation: u64,
    ordinal: u32,
    language: PageHostScriptLanguage,
    kind: PageHostScriptKind,
    outcome: PageHostScriptOutcome,
    source_position: Option<PageHostScriptSourcePosition>,
) -> PageHostScriptReport {
    PageHostScriptReport {
        tab_id,
        document_generation,
        ordinal,
        language,
        kind,
        source_position,
        outcome,
    }
}

pub(super) fn child_debugger_frame(
    tab_id: u64,
    document_generation: u64,
    program: PageHostDebuggerProgram,
    frame: BlueJsPageDebuggerFrame,
) -> PageHostDebuggerFrame {
    PageHostDebuggerFrame {
        tab_id,
        document_generation,
        program,
        code_unit_ordinal: frame.code_unit_ordinal(),
        invocation_serial: frame.invocation_serial(),
    }
}

pub(super) fn child_debugger_linked_frame(
    tab_id: u64,
    document_generation: u64,
    entry_program: PageHostDebuggerProgram,
    dependency_program: PageHostDebuggerProgram,
    frame: BlueJsPageDebuggerLinkedFrame,
) -> PageHostDebuggerLinkedFrame {
    PageHostDebuggerLinkedFrame {
        tab_id,
        document_generation,
        entry_program,
        dependency_program,
        code_unit_ordinal: frame.code_unit_ordinal(),
        invocation_serial: frame.invocation_serial(),
    }
}

pub(super) fn page_host_debugger_value_preview(
    value: VmDebuggerValuePreview,
) -> PageHostDebuggerValuePreview {
    match value {
        VmDebuggerValuePreview::Undefined => PageHostDebuggerValuePreview::Undefined,
        VmDebuggerValuePreview::Null => PageHostDebuggerValuePreview::Null,
        VmDebuggerValuePreview::Bool(value) => PageHostDebuggerValuePreview::Bool(value),
        VmDebuggerValuePreview::NumberBits(bits) => PageHostDebuggerValuePreview::NumberBits(bits),
        VmDebuggerValuePreview::BigIntBytes(bytes) => {
            PageHostDebuggerValuePreview::BigIntBytes(bytes)
        }
        VmDebuggerValuePreview::StringUnits(units) => {
            PageHostDebuggerValuePreview::StringUnits(units)
        }
        VmDebuggerValuePreview::Array(elements) => PageHostDebuggerValuePreview::Array(
            elements
                .into_iter()
                .map(|element| element.map(page_host_debugger_value_preview))
                .collect(),
        ),
        VmDebuggerValuePreview::Record(entries) => PageHostDebuggerValuePreview::Record(
            entries
                .into_iter()
                .map(|(key, value)| {
                    (
                        key.as_code_units().to_vec(),
                        page_host_debugger_value_preview(value),
                    )
                })
                .collect(),
        ),
    }
}

pub(super) fn child_debugger_execution_state(
    tab_id: u64,
    document_generation: u64,
    program: PageHostDebuggerProgram,
    status: ChildDebuggerExecutionStatus,
) -> Option<PageHostDebuggerExecutionState> {
    Some(match status {
        ChildDebuggerExecutionStatus::Pending => PageHostDebuggerExecutionState::Pending,
        ChildDebuggerExecutionStatus::Paused(safe_point) => {
            PageHostDebuggerExecutionState::Paused { safe_point }
        }
        ChildDebuggerExecutionStatus::NestedPaused { frame, safe_point } => {
            PageHostDebuggerExecutionState::NestedPaused {
                frame: child_debugger_frame(tab_id, document_generation, program, frame),
                safe_point,
            }
        }
        ChildDebuggerExecutionStatus::NestedStepRequested { frame, .. } => {
            PageHostDebuggerExecutionState::NestedStepping {
                frame: child_debugger_frame(tab_id, document_generation, program, frame),
            }
        }
        ChildDebuggerExecutionStatus::NestedResumeRequested { frame, .. } => {
            PageHostDebuggerExecutionState::NestedResuming {
                frame: child_debugger_frame(tab_id, document_generation, program, frame),
            }
        }
        ChildDebuggerExecutionStatus::LinkedPaused { .. }
        | ChildDebuggerExecutionStatus::LinkedResumeRequested { .. } => return None,
        ChildDebuggerExecutionStatus::StepRequested
        | ChildDebuggerExecutionStatus::BlueTsSourceStepRequested { .. } => {
            PageHostDebuggerExecutionState::Stepping
        }
        ChildDebuggerExecutionStatus::SourceStepLimitReached(safe_point) => {
            PageHostDebuggerExecutionState::SourceStepLimitReached { safe_point }
        }
        ChildDebuggerExecutionStatus::ResumeRequested => PageHostDebuggerExecutionState::Resuming,
        ChildDebuggerExecutionStatus::Completed => PageHostDebuggerExecutionState::Completed,
    })
}

/// The immutable data-only envelope for debugger contract validation. It is
/// intentionally independent from document/profile limits and cannot be
/// configured over a public or private request.
pub(super) fn debugger_contract_validation_limits() -> ValidationLimits {
    ValidationLimits {
        max_depth: DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_DEPTH,
        max_collection_entries: DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES,
        max_nodes: DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_NODES,
        max_string_bytes: DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES,
    }
}

/// Converts the shared data-only IPC value to the pure BlueTS validation value
/// while applying the fixed debugger limits before a second recursive tree is
/// retained. It never accepts a JavaScript object, function, getter, proxy,
/// host handle, source graph, or compiler configuration.
pub(super) fn debugger_contract_root_kind(
    plan: &ContractPlan,
) -> DebuggerStaticMetadataContractRootKind {
    let mut root = &plan.root;
    // At most one more hop than the retained definition count is attempted.
    // A missing or cyclic reference stays `Reference`, not a plan disclosure.
    for _ in 0..=plan.definitions.len() {
        match root {
            Contract::Reference(name) => {
                let Some(next) = plan.definitions.get(name) else {
                    return DebuggerStaticMetadataContractRootKind::Reference;
                };
                root = next;
            }
            _ => break,
        }
    }
    match root {
        Contract::Null => DebuggerStaticMetadataContractRootKind::Null,
        Contract::Undefined => DebuggerStaticMetadataContractRootKind::Undefined,
        Contract::Boolean => DebuggerStaticMetadataContractRootKind::Boolean,
        Contract::Number => DebuggerStaticMetadataContractRootKind::Number,
        Contract::String => DebuggerStaticMetadataContractRootKind::String,
        Contract::Literal(_) => DebuggerStaticMetadataContractRootKind::Literal,
        Contract::Array(_) => DebuggerStaticMetadataContractRootKind::Array,
        Contract::Tuple(_) | Contract::OptionalTuple { .. } | Contract::RestTuple { .. } => {
            DebuggerStaticMetadataContractRootKind::Tuple
        }
        Contract::Record(_) => DebuggerStaticMetadataContractRootKind::Record,
        Contract::Union(_) => DebuggerStaticMetadataContractRootKind::Union,
        Contract::Intersection(_) => DebuggerStaticMetadataContractRootKind::Intersection,
        Contract::Reference(_) => DebuggerStaticMetadataContractRootKind::Reference,
    }
}

pub(super) fn debugger_contract_value(value: CompilerContractValue) -> Result<ContractValue, ()> {
    fn convert(
        value: CompilerContractValue,
        limits: ValidationLimits,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<ContractValue, ()> {
        if depth > limits.max_depth || *nodes >= limits.max_nodes {
            return Err(());
        }
        *nodes += 1;
        match value {
            CompilerContractValue::Null => Ok(ContractValue::Null),
            CompilerContractValue::Undefined => Ok(ContractValue::Undefined),
            CompilerContractValue::Boolean(value) => Ok(ContractValue::Boolean(value)),
            CompilerContractValue::Number(value) => value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(ContractValue::Number)
                .ok_or(()),
            CompilerContractValue::String(value) => (value.len() <= limits.max_string_bytes)
                .then_some(ContractValue::String(value))
                .ok_or(()),
            CompilerContractValue::Array(values) => {
                if values.len() > limits.max_collection_entries {
                    return Err(());
                }
                values
                    .into_iter()
                    .map(|value| convert(value, limits, depth + 1, nodes))
                    .collect::<Result<Vec<_>, _>>()
                    .map(ContractValue::Array)
            }
            CompilerContractValue::Object(values) => {
                if values.len() > limits.max_collection_entries
                    || values.keys().any(|key| key.len() > limits.max_string_bytes)
                {
                    return Err(());
                }
                values
                    .into_iter()
                    .map(|(key, value)| {
                        convert(value, limits, depth + 1, nodes).map(|value| (key, value))
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()
                    .map(ContractValue::Object)
            }
        }
    }

    let mut nodes = 0;
    convert(value, debugger_contract_validation_limits(), 0, &mut nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn plan(root: Contract, definitions: BTreeMap<String, Contract>) -> ContractPlan {
        ContractPlan {
            id: "test-plan".to_string(),
            root,
            definitions,
            fingerprint: "test-fingerprint".to_string(),
        }
    }

    #[test]
    fn bluets_bridge_category_maps_every_variant() {
        assert_eq!(
            bluets_bridge_category(BridgeError::BlueTs(Vec::new())),
            "BlueTS compilation rejected the page script"
        );
        assert_eq!(
            bluets_bridge_category(BridgeError::PageRuntime(
                BlueJsPageRuntimeError::InvalidConfiguration
            )),
            page_runtime_category(BlueJsPageRuntimeError::InvalidConfiguration)
        );
        assert_eq!(
            bluets_bridge_category(BridgeError::BlueJs(CompileError::ProgramTooLarge)),
            "BlueJS compilation rejected the direct BlueTS page script"
        );
        assert_eq!(
            bluets_bridge_category(BridgeError::BlueJsDebug(
                blueice_bluejs::BlueJsProgramDebugError::GenerationExhausted
            )),
            "BlueJS compilation rejected the direct BlueTS page script"
        );
        assert_eq!(
            bluets_bridge_category(BridgeError::UnsupportedRuntimeTarget {
                span: blueice_bluets::SourceSpan {
                    module: "m".to_string(),
                    start: 0,
                    end: 1,
                },
                message: "unsupported".to_string(),
            }),
            "BlueTS direct lowering rejected the page script"
        );
        assert_eq!(
            bluets_bridge_category(BridgeError::InvalidSourceIdentity("bad".to_string())),
            "BlueTS direct lowering rejected the page script"
        );
        assert_eq!(
            bluets_bridge_category(BridgeError::ProvenanceAttachment("bad".to_string())),
            "BlueTS direct lowering rejected the page script"
        );
        assert_eq!(
            bluets_bridge_category(BridgeError::DebugAttachment(
                blueice_bluets_bluejs::DirectDebugAttachmentError::LanguageVersionMismatch
            )),
            "BlueTS direct lowering rejected the page script"
        );
    }

    #[test]
    fn page_runtime_category_maps_every_group() {
        assert_eq!(
            page_runtime_category(BlueJsPageRuntimeError::BytecodeLimit {
                tab_id: 1,
                limit: 1
            }),
            "JavaScript page resource policy rejected the page script"
        );
        assert_eq!(
            page_runtime_category(BlueJsPageRuntimeError::ProgramLimit {
                tab_id: 1,
                limit: 1
            }),
            "JavaScript page resource policy rejected the page script"
        );
        assert_eq!(
            page_runtime_category(BlueJsPageRuntimeError::RealmLimit { limit: 1 }),
            "JavaScript page resource policy rejected the page script"
        );
        assert_eq!(
            page_runtime_category(BlueJsPageRuntimeError::Runtime(
                RuntimeError::ModuleResolution("missing".to_string())
            )),
            "authorized JavaScript graph rejected the page script"
        );
        assert_eq!(
            page_runtime_category(BlueJsPageRuntimeError::Runtime(
                RuntimeError::InstructionLimit
            )),
            "BlueJS page execution failed"
        );
        assert_eq!(
            page_runtime_category(BlueJsPageRuntimeError::InvalidConfiguration),
            "BlueJS page host rejected the page script"
        );
    }

    #[test]
    fn value_preview_maps_every_shape_without_recursing_incorrectly() {
        assert_eq!(
            page_host_debugger_value_preview(VmDebuggerValuePreview::Undefined),
            PageHostDebuggerValuePreview::Undefined
        );
        assert_eq!(
            page_host_debugger_value_preview(VmDebuggerValuePreview::Null),
            PageHostDebuggerValuePreview::Null
        );
        assert_eq!(
            page_host_debugger_value_preview(VmDebuggerValuePreview::Bool(true)),
            PageHostDebuggerValuePreview::Bool(true)
        );
        assert_eq!(
            page_host_debugger_value_preview(VmDebuggerValuePreview::NumberBits(7)),
            PageHostDebuggerValuePreview::NumberBits(7)
        );
        assert_eq!(
            page_host_debugger_value_preview(VmDebuggerValuePreview::BigIntBytes(vec![1, 2])),
            PageHostDebuggerValuePreview::BigIntBytes(vec![1, 2])
        );
        assert_eq!(
            page_host_debugger_value_preview(VmDebuggerValuePreview::StringUnits(vec![
                b'h' as u16
            ])),
            PageHostDebuggerValuePreview::StringUnits(vec![b'h' as u16])
        );
        assert_eq!(
            page_host_debugger_value_preview(VmDebuggerValuePreview::Array(vec![
                None,
                Some(VmDebuggerValuePreview::Bool(false)),
            ])),
            PageHostDebuggerValuePreview::Array(vec![
                None,
                Some(PageHostDebuggerValuePreview::Bool(false)),
            ])
        );
        assert_eq!(
            page_host_debugger_value_preview(VmDebuggerValuePreview::Record(vec![(
                blueice_bluejs::JsString::from("key"),
                VmDebuggerValuePreview::Null,
            )])),
            PageHostDebuggerValuePreview::Record(vec![(
                blueice_bluejs::JsString::from("key").as_code_units().to_vec(),
                PageHostDebuggerValuePreview::Null,
            )])
        );
    }

    #[test]
    fn contract_root_kind_maps_every_leaf_shape() {
        assert_eq!(
            debugger_contract_root_kind(&plan(Contract::Null, BTreeMap::new())),
            DebuggerStaticMetadataContractRootKind::Null
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(Contract::Undefined, BTreeMap::new())),
            DebuggerStaticMetadataContractRootKind::Undefined
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(Contract::Boolean, BTreeMap::new())),
            DebuggerStaticMetadataContractRootKind::Boolean
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(Contract::Number, BTreeMap::new())),
            DebuggerStaticMetadataContractRootKind::Number
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(Contract::String, BTreeMap::new())),
            DebuggerStaticMetadataContractRootKind::String
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(
                Contract::Literal("x".to_string()),
                BTreeMap::new()
            )),
            DebuggerStaticMetadataContractRootKind::Literal
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(
                Contract::Array(Box::new(Contract::Number)),
                BTreeMap::new()
            )),
            DebuggerStaticMetadataContractRootKind::Array
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(Contract::Tuple(Vec::new()), BTreeMap::new())),
            DebuggerStaticMetadataContractRootKind::Tuple
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(Contract::Union(Vec::new()), BTreeMap::new())),
            DebuggerStaticMetadataContractRootKind::Union
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(
                Contract::Intersection(Vec::new()),
                BTreeMap::new()
            )),
            DebuggerStaticMetadataContractRootKind::Intersection
        );
    }

    #[test]
    fn contract_root_kind_resolves_a_reference_and_rejects_a_missing_one() {
        let mut definitions = BTreeMap::new();
        definitions.insert("Alias".to_string(), Contract::Boolean);
        assert_eq!(
            debugger_contract_root_kind(&plan(
                Contract::Reference("Alias".to_string()),
                definitions
            )),
            DebuggerStaticMetadataContractRootKind::Boolean
        );
        assert_eq!(
            debugger_contract_root_kind(&plan(
                Contract::Reference("Missing".to_string()),
                BTreeMap::new()
            )),
            DebuggerStaticMetadataContractRootKind::Reference
        );
    }

    #[test]
    fn contract_value_converts_every_leaf_shape_and_rejects_non_finite_numbers() {
        assert_eq!(
            debugger_contract_value(CompilerContractValue::Null),
            Ok(ContractValue::Null)
        );
        assert_eq!(
            debugger_contract_value(CompilerContractValue::Undefined),
            Ok(ContractValue::Undefined)
        );
        assert_eq!(
            debugger_contract_value(CompilerContractValue::Number("1.5".to_string())),
            Ok(ContractValue::Number(1.5))
        );
        assert_eq!(
            debugger_contract_value(CompilerContractValue::Number("NaN".to_string())),
            Err(())
        );
        assert_eq!(
            debugger_contract_value(CompilerContractValue::Number("not-a-number".to_string())),
            Err(())
        );
        assert_eq!(
            debugger_contract_value(CompilerContractValue::String("ok".to_string())),
            Ok(ContractValue::String("ok".to_string()))
        );
    }

    #[test]
    fn contract_value_rejects_an_array_over_the_collection_limit() {
        let oversized = vec![
            CompilerContractValue::Null;
            DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES + 1
        ];
        assert_eq!(
            debugger_contract_value(CompilerContractValue::Array(oversized)),
            Err(())
        );
        assert_eq!(
            debugger_contract_value(CompilerContractValue::Array(vec![
                CompilerContractValue::Boolean(true)
            ])),
            Ok(ContractValue::Array(vec![ContractValue::Boolean(true)]))
        );
    }

    #[test]
    fn contract_value_rejects_an_object_over_the_collection_or_key_limit() {
        let mut too_many = BTreeMap::new();
        for index in 0..DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_COLLECTION_ENTRIES + 1 {
            too_many.insert(format!("k{index}"), CompilerContractValue::Null);
        }
        assert_eq!(
            debugger_contract_value(CompilerContractValue::Object(too_many)),
            Err(())
        );
        let mut oversized_key = BTreeMap::new();
        oversized_key.insert(
            "k".repeat(DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_STRING_BYTES + 1),
            CompilerContractValue::Null,
        );
        assert_eq!(
            debugger_contract_value(CompilerContractValue::Object(oversized_key)),
            Err(())
        );
    }

    #[test]
    fn contract_value_rejects_a_value_deeper_than_the_depth_limit() {
        let mut value = CompilerContractValue::Null;
        for _ in 0..=DEBUGGER_STATIC_METADATA_CONTRACT_VALIDATION_MAX_DEPTH + 1 {
            value = CompilerContractValue::Array(vec![value]);
        }
        assert_eq!(debugger_contract_value(value), Err(()));
    }
}
