// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Owner-certified diagnostic positions attached to immutable compiled code.
//! Instruction ranges are explicit: an unmapped gap never inherits a nearby
//! position. Coordinates are one-based; columns count original UTF-16 units.

use crate::{BlueJsSourceIdentity, Bytecode};
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

pub use crate::program_debug::ast_ids::DiagnosticNode as DiagnosticSourceNode;

/// Enumerate one owner-provided AST's diagnostic ordinals without installing
/// code or exposing a live program identity. Slot zero is the unmapped program
/// root; binding- and assignment-pattern slots follow the public AST prefix.
/// The returned total includes all slots, including the root.
pub fn visit_diagnostic_source_nodes(
    program: &crate::BlueJsProgramV1,
    visit: &mut dyn FnMut(DiagnosticSourceNode<'_>, usize),
) -> usize {
    let body = match program {
        crate::BlueJsProgramV1::Script(program) => &program.body,
        crate::BlueJsProgramV1::Module(module) => &module.body,
    };
    crate::program_debug::ast_ids::visit_program_nodes(body, visit)
}

pub const MAX_DIAGNOSTIC_SOURCE_NAME_BYTES: usize = 8 * 1024;
pub const MAX_DIAGNOSTIC_SOURCE_RANGES: usize = 65_536;
pub const MAX_DIAGNOSTIC_SOURCE_MAP_BYTES: usize = 8 * 1024 * 1024;

/// Convert exact UTF-8 byte boundaries in one scan, retaining request order.
/// CRLF is one line break; CR, LF, LS and PS all reset UTF-16 columns.
pub fn source_positions(source: &str, offsets: &[usize]) -> Vec<Option<(u32, u32)>> {
    let mut requested = BTreeMap::new();
    for &offset in offsets {
        if offset <= source.len() && source.is_char_boundary(offset) {
            requested.insert(offset, None);
        }
    }
    let mut position = Some((1u32, 1u32));
    let mut previous_cr = false;
    for (offset, character) in source.char_indices() {
        if let Some(slot) = requested.get_mut(&offset) {
            *slot = position;
        }
        position = position.and_then(|(line, column)| match character {
            '\r' | '\u{2028}' | '\u{2029}' => Some((line.checked_add(1)?, 1)),
            '\n' if previous_cr => Some((line, 1)),
            '\n' => Some((line.checked_add(1)?, 1)),
            _ => Some((line, column.checked_add(character.len_utf16() as u32)?)),
        });
        previous_cr = character == '\r';
    }
    if let Some(slot) = requested.get_mut(&source.len()) {
        *slot = position;
    }
    offsets
        .iter()
        .map(|offset| requested.get(offset).copied().flatten())
        .collect()
}

/// Copied source information. It contains no source text or live VM handle.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DiagnosticSourceLocation {
    pub filename: String,
    pub line: u32,
    pub column: u32,
}

/// Provenance displaced while cleanup runs user code. The epoch belongs to
/// the saved throw too: an ignored cleanup failure must not masquerade as a
/// new propagation of the original error.
#[derive(Clone, Default)]
pub(crate) struct ThrowOrigin {
    pub(crate) source: Option<DiagnosticSourceLocation>,
    pub(crate) debugger_site: Option<crate::VmDebuggerThrowSite>,
    pub(crate) epoch: u64,
}

impl ThrowOrigin {
    pub(crate) fn payload_bytes(&self) -> usize {
        self.source
            .as_ref()
            .map_or(0, |source| source.filename.capacity())
    }
}

/// One compiler-certified half-open instruction range in a code unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticSourceRange {
    pub code_unit_ordinal: u32,
    pub start: u32,
    pub end: u32,
    pub location: DiagnosticSourceLocation,
}

/// Temporary compiler provenance consumed by a page owner's source-map builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticInstructionOwner {
    pub bytecode_offset: u32,
    /// Diagnostic inventory index: the public statement/expression AST prefix
    /// followed by binding- and assignment-pattern slots. Pattern slots grant
    /// no debugger authority.
    pub ast_node_ordinal: Option<u32>,
}

/// An embedding owner's map for one exact authorized source artifact.
/// Binding validates every instruction boundary before publishing any entry.
#[derive(Debug, Clone)]
pub struct DiagnosticSourceMap {
    pub(crate) source: BlueJsSourceIdentity,
    ranges: Vec<DiagnosticSourceRange>,
    generation: Option<u64>,
    payload_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSourceMapError {
    UnknownProgram,
    SourceMismatch,
    AlreadyBound,
    InvalidLocation,
    InvalidRange,
    OverlappingRanges,
    UnknownCodeUnit,
    InvalidInstructionBoundary,
    LimitExceeded,
}

impl fmt::Display for DiagnosticSourceMapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid diagnostic source map: {self:?}")
    }
}

impl std::error::Error for DiagnosticSourceMapError {}

impl DiagnosticSourceMap {
    /// Build bounded instruction ranges from exact source-only AST positions.
    /// Missing slots stay unmapped, including through adjacent mapped owners.
    /// The caller certifies the positions; binding still validates the exact
    /// source identity, live generation and every instruction boundary.
    pub fn from_node_positions(
        source: BlueJsSourceIdentity,
        bytecode: &Bytecode,
        filename: &str,
        positions: &[Option<(u32, u32)>],
    ) -> Result<Self, DiagnosticSourceMapError> {
        if positions.len() != bytecode.diagnostic_source_node_count() {
            return Err(DiagnosticSourceMapError::SourceMismatch);
        }
        let mut entries = Vec::<DiagnosticSourceRange>::new();
        let mut filename_bytes = 0usize;
        fn collect(
            code: &Bytecode,
            ordinal: &mut u32,
            filename: &str,
            positions: &[Option<(u32, u32)>],
            entries: &mut Vec<DiagnosticSourceRange>,
            filename_bytes: &mut usize,
        ) -> Result<(), DiagnosticSourceMapError> {
            let unit = *ordinal;
            *ordinal = ordinal
                .checked_add(1)
                .ok_or(DiagnosticSourceMapError::LimitExceeded)?;
            let owners = code.diagnostic_instruction_owners();
            for (index, owner) in owners.iter().enumerate() {
                let Some((line, column)) = owner
                    .ast_node_ordinal
                    .and_then(|node| positions.get(node as usize))
                    .copied()
                    .flatten()
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
                        && previous.location.line == line
                        && previous.location.column == column
                }) {
                    previous.end = end;
                    continue;
                }
                let next = entries
                    .len()
                    .checked_add(1)
                    .filter(|count| *count <= MAX_DIAGNOSTIC_SOURCE_RANGES)
                    .ok_or(DiagnosticSourceMapError::LimitExceeded)?;
                let capacity = if next > entries.capacity() {
                    entries.capacity().saturating_mul(2).max(4)
                } else {
                    entries.capacity()
                };
                capacity
                    .checked_mul(std::mem::size_of::<DiagnosticSourceRange>())
                    .and_then(|bytes| bytes.checked_add(*filename_bytes))
                    .and_then(|bytes| bytes.checked_add(filename.len()))
                    .filter(|bytes| *bytes <= MAX_DIAGNOSTIC_SOURCE_MAP_BYTES)
                    .ok_or(DiagnosticSourceMapError::LimitExceeded)?;
                *filename_bytes = filename_bytes
                    .checked_add(filename.len())
                    .ok_or(DiagnosticSourceMapError::LimitExceeded)?;
                entries.push(DiagnosticSourceRange {
                    code_unit_ordinal: unit,
                    start,
                    end,
                    location: DiagnosticSourceLocation {
                        filename: filename.to_owned(),
                        line,
                        column,
                    },
                });
            }
            for child in code.child_code_units() {
                collect(child, ordinal, filename, positions, entries, filename_bytes)?;
            }
            Ok(())
        }
        collect(
            bytecode,
            &mut 0,
            filename,
            positions,
            &mut entries,
            &mut filename_bytes,
        )?;
        Self::new(source, entries)
    }

    pub fn new(
        source: BlueJsSourceIdentity,
        mut ranges: Vec<DiagnosticSourceRange>,
    ) -> Result<Self, DiagnosticSourceMapError> {
        use DiagnosticSourceMapError as Error;
        if ranges.len() > MAX_DIAGNOSTIC_SOURCE_RANGES {
            return Err(Error::LimitExceeded);
        }
        let mut bytes = std::mem::size_of::<Self>()
            .saturating_add(source.owned_identity_bytes())
            .saturating_add(
                ranges
                    .capacity()
                    .saturating_mul(std::mem::size_of::<DiagnosticSourceRange>()),
            );
        for range in &ranges {
            let location = &range.location;
            if location.filename.is_empty()
                || location.filename.len() > MAX_DIAGNOSTIC_SOURCE_NAME_BYTES
                || location.line == 0
                || location.column == 0
            {
                return Err(Error::InvalidLocation);
            }
            if range.start >= range.end {
                return Err(Error::InvalidRange);
            }
            bytes = bytes.saturating_add(location.filename.capacity());
        }
        if bytes > MAX_DIAGNOSTIC_SOURCE_MAP_BYTES {
            return Err(Error::LimitExceeded);
        }
        ranges.sort_by_key(|range| (range.code_unit_ordinal, range.start));
        if ranges.windows(2).any(|pair| {
            pair[0].code_unit_ordinal == pair[1].code_unit_ordinal && pair[0].end > pair[1].start
        }) {
            return Err(Error::OverlappingRanges);
        }
        Ok(Self {
            source,
            ranges,
            generation: None,
            payload_bytes: bytes,
        })
    }

    /// Charge once per installed program; all its code units share this data.
    pub fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    pub(crate) fn bind(mut self, code: &mut Bytecode) -> Result<(), DiagnosticSourceMapError> {
        use DiagnosticSourceMapError as Error;
        fn units(code: &Bytecode, output: &mut BTreeMap<u32, (usize, Vec<u32>)>) {
            if let Some(ordinal) = code.debugger_code_unit_ordinal {
                output.insert(
                    ordinal,
                    (
                        code.bytes().len(),
                        code.instructions()
                            .map(|instruction| instruction.offset as u32)
                            .collect(),
                    ),
                );
            }
            for child in &code.functions {
                units(child, output);
            }
        }
        if code.diagnostic_source_map.is_some() {
            return Err(Error::AlreadyBound);
        }
        let generation = code
            .debugger_program_generation
            .ok_or(Error::UnknownProgram)?;
        let mut code_units = BTreeMap::new();
        units(code, &mut code_units);
        for range in &self.ranges {
            let (length, offsets) = code_units
                .get(&range.code_unit_ordinal)
                .ok_or(Error::UnknownCodeUnit)?;
            if offsets.binary_search(&range.start).is_err()
                || (range.end as usize != *length && offsets.binary_search(&range.end).is_err())
            {
                return Err(Error::InvalidInstructionBoundary);
            }
        }
        self.generation = Some(generation);
        fn attach(code: &mut Bytecode, map: &Rc<DiagnosticSourceMap>) {
            code.diagnostic_source_map = Some(map.clone());
            code.diagnostic_ast_owners = Vec::new();
            for child in &mut code.functions {
                attach(Rc::make_mut(child), map);
            }
        }
        attach(code, &Rc::new(self));
        Ok(())
    }

    fn location(&self, unit: u32, offset: u32) -> Option<&DiagnosticSourceLocation> {
        let index = self
            .ranges
            .partition_point(|range| (range.code_unit_ordinal, range.start) <= (unit, offset))
            .checked_sub(1)?;
        let range = &self.ranges[index];
        (range.code_unit_ordinal == unit && offset < range.end).then_some(&range.location)
    }
}

impl Bytecode {
    /// Source-node inventory size, including pattern-only diagnostic slots.
    /// These slots never extend the debugger's executable-AST inventory.
    pub fn diagnostic_source_node_count(&self) -> usize {
        self.diagnostic_source_node_count
    }

    pub fn diagnostic_instruction_owners(&self) -> &[DiagnosticInstructionOwner] {
        &self.diagnostic_ast_owners
    }

    /// A certified mapping for this exact code unit, or an explicit absence.
    pub fn diagnostic_source_location(&self, offset: u32) -> Option<&DiagnosticSourceLocation> {
        let map = self.diagnostic_source_map.as_ref()?;
        if map.generation != self.debugger_program_generation {
            return None;
        }
        map.location(self.debugger_code_unit_ordinal?, offset)
    }

    pub fn diagnostic_source_map_bytes(&self) -> usize {
        self.diagnostic_source_map
            .as_ref()
            .map_or(0, |map| map.payload_bytes())
    }
}

/// Native execution context, restored when a nested interpreter returns.
pub(crate) struct ActiveDiagnosticFrame {
    map: Rc<DiagnosticSourceMap>,
    unit: u32,
    pub(crate) offset: u32,
}

impl ActiveDiagnosticFrame {
    pub(crate) fn new(code: &Bytecode, offset: usize) -> Option<Self> {
        let map = code.diagnostic_source_map.as_ref()?;
        if map.generation != code.debugger_program_generation {
            return None;
        }
        Some(Self {
            map: map.clone(),
            unit: code.debugger_code_unit_ordinal?,
            offset: u32::try_from(offset).ok()?,
        })
    }

    pub(crate) fn location(&self) -> Option<&DiagnosticSourceLocation> {
        self.map.location(self.unit, self.offset)
    }
}
