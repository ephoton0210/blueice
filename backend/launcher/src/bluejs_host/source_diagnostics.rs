// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Positions from the owner's parsed source and compiler-recorded ranges.

use super::*;
use blueice_bluejs::diagnostics::{
    source_positions, DiagnosticSourceMap, DiagnosticSourceMapError,
};

fn invalid_source() -> BlueJsPageRuntimeError {
    BlueJsPageRuntimeError::DiagnosticSourceMap(DiagnosticSourceMapError::SourceMismatch)
}

fn limit() -> BlueJsPageRuntimeError {
    BlueJsPageRuntimeError::DiagnosticSourceMap(DiagnosticSourceMapError::LimitExceeded)
}

fn diagnostic_spans(
    compiled: &blueice_bluejs::BlueJsCompiledProgram,
    source: &PageHostSource,
) -> Result<Vec<Option<blueice_bluejs::ParsedDiagnosticSpan>>, BlueJsPageRuntimeError> {
    if compiled.source().canonical_module_id() != source.canonical_module_id
        || compiled.source().source_hash() != source.source_hash
        || source.source_hash != page_host::source_hash(&source.source)
    {
        return Err(invalid_source());
    }
    let spans = match compiled.ast_nodes().first().map(|node| node.kind()) {
        Some(blueice_bluejs::BlueJsAstNodeKind::Script) => {
            blueice_bluejs::parse_with_diagnostic_node_spans(&source.source).map(|(_, spans)| spans)
        }
        Some(blueice_bluejs::BlueJsAstNodeKind::Module) => {
            blueice_bluejs::parse_module_with_diagnostic_node_spans(&source.source)
                .map(|(_, spans)| spans)
        }
        _ => return Err(invalid_source()),
    }
    .map_err(|_| invalid_source())?;
    if compiled.bytecode().diagnostic_source_node_count() != spans.len() {
        return Err(invalid_source());
    }
    Ok(spans)
}

pub(super) fn javascript_diagnostic_source_map(
    registry: &blueice_bluejs::BlueJsProgramRegistry,
    handle: BlueJsProgramHandle,
    source: &PageHostSource,
) -> Result<DiagnosticSourceMap, BlueJsPageRuntimeError> {
    let compiled = registry
        .get(handle)
        .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
    let spans = diagnostic_spans(compiled, source)?;
    let offsets = spans
        .iter()
        .map(|span| span.map_or(0, |span| span.start_byte))
        .collect::<Vec<_>>();
    let positions = source_positions(&source.source, &offsets)
        .into_iter()
        .zip(&spans)
        .map(|(position, span)| span.and(position))
        .collect::<Vec<_>>();
    bind_positions(compiled, &source.canonical_module_id, &positions)
}

pub(super) fn emitted_diagnostic_source_map(
    registry: &blueice_bluejs::BlueJsProgramRegistry,
    handle: BlueJsProgramHandle,
    bound: &BoundEmittedMap,
) -> Result<DiagnosticSourceMap, BlueJsPageRuntimeError> {
    let original = bound.original.as_ref().ok_or_else(invalid_source)?;
    if bound.map.sources.as_slice() != [original.canonical_module_id.as_str()]
        || bound.map.sources_content.len() != 1
        || !bound.map.source_root.is_empty()
        || page_host::source_hash(&bound.map.sources_content[0]) != original.source_hash
    {
        return Err(invalid_source());
    }
    let compiled = registry
        .get(handle)
        .map_err(BlueJsPageRuntimeError::ProgramRegistry)?;
    let spans = diagnostic_spans(compiled, &bound.generated)?;
    if spans.len() > BoundEmittedMap::MAX_SEGMENTS {
        return Err(limit());
    }
    let offsets = spans
        .iter()
        .map(|span| span.map_or(0, |span| span.start_byte))
        .collect::<Vec<_>>();
    let mut queries = offsets
        .iter()
        .map(|offset| (*offset, None))
        .collect::<BTreeMap<_, _>>();
    visit_map_coordinates(&bound.generated.source, |offset, line, column| {
        if let Some(slot) = queries.get_mut(&offset) {
            *slot = Some((line, column));
        }
    });
    let queries = offsets
        .iter()
        .map(|offset| queries[offset].ok_or_else(invalid_source))
        .collect::<Result<Vec<_>, _>>()?;
    let mapped = bound
        .map
        .original_positions_at_exact(
            &bound.generated.source,
            &queries,
            BoundEmittedMap::MAX_SEGMENTS,
            BoundEmittedMap::MAX_RETAINED_BYTES,
        )
        .map_err(|_| invalid_source())?;
    let mut requested = mapped
        .iter()
        .flatten()
        .map(|position| ((position.line, position.column_utf16), None))
        .collect::<BTreeMap<_, _>>();
    let source = &bound.map.sources_content[0];
    visit_map_coordinates(source, |offset, line, column| {
        if let Some(slot) = requested.get_mut(&(line, column)) {
            *slot = Some(offset);
        }
    });
    let original_offsets = mapped
        .iter()
        .map(|position| {
            position.and_then(|position| requested[&(position.line, position.column_utf16)])
        })
        .collect::<Vec<_>>();
    let coordinates = source_positions(
        source,
        &original_offsets
            .iter()
            .map(|offset| offset.unwrap_or(0))
            .collect::<Vec<_>>(),
    );
    let positions = coordinates
        .into_iter()
        .zip(original_offsets)
        .zip(&spans)
        .map(|((position, offset), span)| span.and(offset).and(position))
        .collect::<Vec<_>>();
    bind_positions(compiled, &original.canonical_module_id, &positions)
}

/// Match the compiler emitter's CR/LF and UTF-16 coordinates. Convert through
/// exact UTF-8 boundaries before publishing ECMAScript CR/LF/LS/PS coordinates.
fn visit_map_coordinates(text: &str, mut visit: impl FnMut(usize, usize, usize)) {
    let (mut line, mut column) = (0, 0);
    let mut characters = text.char_indices().peekable();
    while let Some((offset, character)) = characters.next() {
        visit(offset, line, column);
        if character == '\n'
            || (character == '\r' && characters.peek().is_none_or(|(_, next)| *next != '\n'))
        {
            line += 1;
            column = 0;
        } else {
            column += character.len_utf16();
        }
    }
    visit(text.len(), line, column);
}

fn bind_positions(
    compiled: &blueice_bluejs::BlueJsCompiledProgram,
    filename: &str,
    positions: &[Option<(u32, u32)>],
) -> Result<DiagnosticSourceMap, BlueJsPageRuntimeError> {
    DiagnosticSourceMap::from_node_positions(
        compiled.source().clone(),
        compiled.bytecode(),
        filename,
        positions,
    )
    .map_err(BlueJsPageRuntimeError::DiagnosticSourceMap)
}
