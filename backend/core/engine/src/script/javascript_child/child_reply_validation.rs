// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn has_duplicate_child_programs(programs: &[PageHostDebuggerProgram]) -> bool {
    let mut seen = BTreeSet::new();
    programs.iter().any(|program| !seen.insert(*program))
}

pub(super) fn has_duplicate_child_safe_points(safe_points: &[PageHostDebuggerSafePoint]) -> bool {
    let mut seen = BTreeSet::new();
    safe_points
        .iter()
        .any(|safe_point| !safe_point.is_well_formed() || !seen.insert(*safe_point))
}

pub(super) fn has_duplicate_child_static_metadata(
    metadata: &[PageHostDebuggerMetadataHandle],
) -> bool {
    let mut seen = BTreeSet::new();
    metadata.iter().any(|metadata| !seen.insert(*metadata))
}

pub(super) fn has_duplicate_child_static_metadata_source_ids(
    sources: &[blueice_ipc::page_host::PageHostDebuggerBlueTsMetadataSourceId],
) -> bool {
    let mut seen = BTreeSet::new();
    sources.iter().any(|source| !seen.insert(source.source_id))
}

pub(super) fn has_duplicate_child_static_metadata_type_ids(
    types: &[blueice_ipc::page_host::PageHostDebuggerBlueTsMetadataTypeId],
) -> bool {
    let mut seen = BTreeSet::new();
    types
        .iter()
        .any(|static_type| !seen.insert(static_type.type_id))
}

pub(super) fn has_duplicate_child_static_metadata_symbol_ids(
    symbols: &[blueice_ipc::page_host::PageHostDebuggerBlueTsMetadataSymbolId],
) -> bool {
    let mut seen = BTreeSet::new();
    symbols.iter().any(|symbol| !seen.insert(symbol.symbol_id))
}

pub(super) fn valid_child_static_metadata_symbol_location(
    location: PageHostDebuggerBlueTsMetadataSymbolLocation,
    symbol_id: u32,
    source_id: u32,
) -> bool {
    location.symbol_id == symbol_id
        && location.source_id == source_id
        && location
            .coordinates
            .is_well_formed_for_range(location.start_byte, location.end_byte)
}

pub(super) fn valid_child_static_metadata_symbol_type(
    symbol_type: PageHostDebuggerBlueTsMetadataSymbolType,
    symbol_id: u32,
    type_id: u32,
) -> bool {
    symbol_type.symbol_id == symbol_id && symbol_type.type_id == type_id
}

pub(super) fn valid_child_static_metadata_symbol_contract(
    symbol_contract: PageHostDebuggerBlueTsMetadataSymbolContract,
    symbol_id: u32,
    contract_id: u32,
) -> bool {
    symbol_contract.symbol_id == symbol_id && symbol_contract.contract_id == contract_id
}

pub(super) fn has_duplicate_child_static_metadata_contract_ids(
    contracts: &[blueice_ipc::page_host::PageHostDebuggerBlueTsMetadataContractId],
) -> bool {
    let mut seen = BTreeSet::new();
    contracts
        .iter()
        .any(|contract| !seen.insert(contract.contract_id))
}

pub(super) fn remint_child_debugger_value(
    preview: PageHostDebuggerValuePreview,
) -> JavaScriptPageDebuggerValuePreview {
    match preview {
        PageHostDebuggerValuePreview::Undefined => JavaScriptPageDebuggerValuePreview::Undefined,
        PageHostDebuggerValuePreview::Null => JavaScriptPageDebuggerValuePreview::Null,
        PageHostDebuggerValuePreview::Bool(value) => {
            JavaScriptPageDebuggerValuePreview::Bool(value)
        }
        PageHostDebuggerValuePreview::NumberBits(bits) => {
            JavaScriptPageDebuggerValuePreview::NumberBits(bits)
        }
        PageHostDebuggerValuePreview::BigIntBytes(bytes) => {
            JavaScriptPageDebuggerValuePreview::BigIntBytes(bytes)
        }
        PageHostDebuggerValuePreview::StringUnits(units) => {
            JavaScriptPageDebuggerValuePreview::StringUnits(units)
        }
        PageHostDebuggerValuePreview::Array(elements) => JavaScriptPageDebuggerValuePreview::Array(
            elements
                .into_iter()
                .map(|element| element.map(remint_child_debugger_value))
                .collect(),
        ),
        PageHostDebuggerValuePreview::Record(entries) => {
            JavaScriptPageDebuggerValuePreview::Record(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, remint_child_debugger_value(value)))
                    .collect(),
            )
        }
    }
}

pub(super) fn valid_child_debugger_stack_snapshot(
    snapshot: &PageHostDebuggerStackSnapshot,
    child_frame: Option<PageHostDebuggerFrame>,
    top_code_unit: u32,
    top_offset: u32,
    max_frames: u32,
    max_scope_entries: u32,
) -> bool {
    let nested = child_frame.is_some();
    let expected_frames = if nested && max_frames > 1 { 2 } else { 1 };
    if snapshot.frames.len() != expected_frames
        || snapshot.stack_truncated != (nested && max_frames == 1)
        || snapshot.frames[0].code_unit_ordinal != top_code_unit
        || snapshot.frames[0].bytecode_offset != top_offset
        || (expected_frames == 2 && snapshot.frames[1].code_unit_ordinal != 0)
    {
        return false;
    }
    snapshot.frames.iter().all(|frame| {
        frame.scope_entries.len() <= max_scope_entries as usize
            && (!frame.scope_truncated || frame.scope_entries.len() == max_scope_entries as usize)
            && frame
                .scope_entries
                .windows(2)
                .all(|pair| pair[0].scope_depth <= pair[1].scope_depth)
    })
}

pub(super) fn validate_child_safe_point_reply<C: PageHostClient>(
    executor: &mut OutOfProcessJavaScriptPageExecutor<C>,
    tab_id: TabId,
    document_generation: u64,
    safe_point: PageHostDebuggerSafePoint,
) -> Result<(), JavaScriptPageDebuggerError> {
    let reply = executor
        .child
        .validate_debugger_safe_point(tab_id.as_u64(), document_generation, safe_point)
        .map_err(|_| JavaScriptPageDebuggerError::NoLiveRealm)?;
    match reply {
        PageHostReply::DebuggerSafePointValidated {
            tab_id: reply_tab_id,
            document_generation: reply_generation,
            safe_point: reply_safe_point,
        } if reply_tab_id == tab_id.as_u64()
            && reply_generation == document_generation
            && reply_safe_point == safe_point =>
        {
            Ok(())
        }
        PageHostReply::Error { .. } => Err(child_debugger_reply_error(&reply)),
        _ => Err(JavaScriptPageDebuggerError::NoLiveRealm),
    }
}

pub(super) fn child_debugger_reply_error(reply: &PageHostReply) -> JavaScriptPageDebuggerError {
    match reply {
        PageHostReply::Error {
            code: PageHostErrorCode::ResourceLimit,
            ..
        } => JavaScriptPageDebuggerError::ResourceLimit,
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidDebuggerState,
            ..
        } => JavaScriptPageDebuggerError::InvalidExecutionState,
        // Treat every other unexpected/private-child response as a lost live
        // realm. The public debugger must not infer a child registry state,
        // source identity, VM result, or a usable fallback target from it.
        _ => JavaScriptPageDebuggerError::NoLiveRealm,
    }
}

/// An exact, already-receipted relation or safe-point source binding can be
/// absent without losing the live realm. The child uses a generic private
/// InvalidRequest for an unbound or mismatched target; expose only public
/// InvalidTarget.
pub(super) fn child_static_metadata_relation_reply_error(
    reply: &PageHostReply,
) -> JavaScriptPageDebuggerError {
    match reply {
        PageHostReply::Error {
            code: PageHostErrorCode::InvalidRequest,
            ..
        } => JavaScriptPageDebuggerError::UnknownProgram,
        _ => child_debugger_reply_error(reply),
    }
}
