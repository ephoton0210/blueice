// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Positions from the owner's parsed source and compiler-recorded ranges.

use super::*;
use blueice_bluejs::diagnostics::{
    source_positions, DiagnosticSourceLocation, DiagnosticSourceMap, DiagnosticSourceMapError,
    DiagnosticSourceRange,
};

pub(super) fn javascript_diagnostic_source_map(
    registry: &blueice_bluejs::BlueJsProgramRegistry,
    handle: BlueJsProgramHandle,
    source: &PageHostSource,
) -> Result<DiagnosticSourceMap, BlueJsPageRuntimeError> {
    let invalid =
        || BlueJsPageRuntimeError::DiagnosticSourceMap(DiagnosticSourceMapError::SourceMismatch);
    let compiled = registry
        .get(handle)
        .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
    if compiled.source().canonical_module_id() != source.canonical_module_id
        || compiled.source().source_hash() != source.source_hash
    {
        return Err(invalid());
    }
    let (_, spans) =
        blueice_bluejs::parse_with_diagnostic_node_spans(&source.source).map_err(|_| invalid())?;
    if compiled.bytecode().diagnostic_source_node_count() != spans.len() {
        return Err(invalid());
    }
    let offsets = spans
        .iter()
        .filter_map(|span| span.map(|span| span.start_byte))
        .collect::<Vec<_>>();
    let positions = source_positions(&source.source, &offsets);
    let mut positions = positions.into_iter();
    let locations = spans
        .iter()
        .map(|span| {
            span.and_then(|_| positions.next().flatten())
                .map(|(line, column)| DiagnosticSourceLocation {
                    filename: source.canonical_module_id.clone(),
                    line,
                    column,
                })
        })
        .collect::<Vec<_>>();
    let mut entries: Vec<DiagnosticSourceRange> = Vec::new();
    fn collect(
        code: &blueice_bluejs::Bytecode,
        ordinal: &mut u32,
        locations: &[Option<DiagnosticSourceLocation>],
        entries: &mut Vec<DiagnosticSourceRange>,
    ) {
        let unit = *ordinal;
        *ordinal += 1;
        let owners = code.diagnostic_instruction_owners();
        for (index, owner) in owners.iter().enumerate() {
            let Some(location) = owner
                .ast_node_ordinal
                .and_then(|node| locations.get(node as usize))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let start = owner.bytecode_offset;
            let end = owners
                .get(index + 1)
                .map_or(code.bytes().len() as u32, |next| next.bytecode_offset);
            if let Some(previous) = entries.last_mut().filter(|previous| {
                previous.code_unit_ordinal == unit
                    && previous.end == start
                    && previous.location == *location
            }) {
                previous.end = end;
            } else {
                entries.push(DiagnosticSourceRange {
                    code_unit_ordinal: unit,
                    start,
                    end,
                    location: location.clone(),
                });
            }
        }
        for child in code.child_code_units() {
            collect(child, ordinal, locations, entries);
        }
    }
    collect(compiled.bytecode(), &mut 0, &locations, &mut entries);
    DiagnosticSourceMap::new(compiled.source().clone(), entries)
        .map_err(BlueJsPageRuntimeError::DiagnosticSourceMap)
}
