// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn source_identity(
    sources: &[BridgeSource],
) -> Result<bluejs::BlueJsSourceIdentity, BridgeError> {
    let executable_sources = sources
        .iter()
        .filter(|source| !source.module.ends_with(".d.ts"))
        .collect::<Vec<_>>();
    let [source] = executable_sources.as_slice() else {
        return Err(BridgeError::InvalidSourceIdentity(
            "a direct script or module must retain exactly one executable source".to_string(),
        ));
    };
    bluejs::BlueJsSourceIdentity::new(source.module.clone(), source.content_hash.clone())
        .map_err(BridgeError::BlueJsDebug)
}

pub(super) struct DirectArtifactProgram<'a> {
    pub(super) program: &'a bluejs::BlueJsProgramV1,
    pub(super) expected_bytecode: Option<&'a bluejs::Bytecode>,
}

pub(super) fn attach_direct_program(
    registry: &mut bluejs::BlueJsProgramRegistry,
    sources: &[BridgeSource],
    program: &bluejs::BlueJsProgramV1,
    provenance: &[LoweringProvenance],
    compiler_options_fingerprint: &str,
    static_info: &BlueTsDebugInfo,
) -> Result<DirectProgramAttachment, BridgeError> {
    let source = source_identity(sources)?;
    let handle = registry
        .install(source, program)
        .map_err(BridgeError::BlueJsDebug)?;
    match attach_existing_direct_program(
        registry,
        handle,
        sources,
        DirectArtifactProgram {
            program,
            expected_bytecode: None,
        },
        provenance,
        compiler_options_fingerprint,
        static_info,
    ) {
        Ok(attachment) => Ok(attachment),
        Err(error) => {
            registry.invalidate(handle);
            Err(error)
        }
    }
}

pub(super) fn attach_existing_direct_program(
    registry: &bluejs::BlueJsProgramRegistry,
    handle: bluejs::BlueJsProgramHandle,
    sources: &[BridgeSource],
    artifact: DirectArtifactProgram<'_>,
    provenance: &[LoweringProvenance],
    compiler_options_fingerprint: &str,
    static_info: &BlueTsDebugInfo,
) -> Result<DirectProgramAttachment, BridgeError> {
    let source = source_identity(sources)?;
    let compiled = registry.get(handle).map_err(BridgeError::BlueJsDebug)?;
    if compiled.source() != &source {
        return Err(BridgeError::ProvenanceAttachment(
            "the live program source identity does not match this direct artifact".to_string(),
        ));
    }
    if artifact
        .expected_bytecode
        .is_some_and(|expected| !bytecode_matches(expected, compiled.bytecode()))
    {
        return Err(BridgeError::ProvenanceAttachment(
            "the live program bytecode does not match this direct artifact".to_string(),
        ));
    }
    let nodes = compiled
        .ast_nodes()
        .iter()
        .filter(|node| node.is_top_level_statement())
        .map(|node| node.id())
        .collect::<Vec<_>>();
    if nodes.len() != provenance.len() {
        return Err(BridgeError::ProvenanceAttachment(format!(
            "BlueTS retained {} top-level lowering spans but BlueJS generated {} top-level statements",
            provenance.len(),
            nodes.len()
        )));
    }
    let attached_provenance = provenance
        .iter()
        .zip(nodes)
        .map(|(provenance, node_id)| {
            let safe_point = match registry.safe_point_for_ast_node(handle, node_id) {
                Ok(safe_point) => DirectSafePointBinding::Bound(safe_point),
                Err(bluejs::BlueJsProgramDebugError::AstNodeUnbound) => {
                    DirectSafePointBinding::Unbound
                }
                Err(error) => return Err(BridgeError::BlueJsDebug(error)),
            };
            Ok(AttachedLoweringProvenance {
                source: provenance.source.clone(),
                kind: provenance.kind,
                location: provenance.location,
                node_id,
                safe_point,
            })
        })
        .collect::<Result<Vec<_>, BridgeError>>()?;
    let safe_point_map = build_safe_point_map(
        handle,
        compiler_options_fingerprint,
        sources,
        &attached_provenance,
        compiled,
    )?;
    let root_symbol_slots = build_root_symbol_slots(
        handle,
        compiled,
        artifact.program,
        sources,
        &attached_provenance,
        compiler_options_fingerprint,
        static_info,
    );
    Ok(DirectProgramAttachment {
        handle,
        provenance: attached_provenance,
        safe_point_map,
        root_symbol_slots,
    })
}

fn bytecode_matches(expected: &bluejs::Bytecode, actual: &bluejs::Bytecode) -> bool {
    expected.bytes() == actual.bytes()
        && expected.debugger_binding_layout_matches(actual)
        && expected.constants() == actual.constants()
        && expected.root_statement_offsets() == actual.root_statement_offsets()
        && expected.root_statement_ranges() == actual.root_statement_ranges()
        && expected.root_function_child_indices() == actual.root_function_child_indices()
        && expected.root_declaration_binding_slots() == actual.root_declaration_binding_slots()
        && expected
            .child_code_units()
            .zip(actual.child_code_units())
            .all(|(expected, actual)| bytecode_matches(expected, actual))
        && expected.child_code_units().count() == actual.child_code_units().count()
}

fn build_root_symbol_slots(
    handle: bluejs::BlueJsProgramHandle,
    compiled: &bluejs::BlueJsCompiledProgram,
    program: &bluejs::BlueJsProgramV1,
    sources: &[BridgeSource],
    provenance: &[AttachedLoweringProvenance],
    compiler_options_fingerprint: &str,
    static_info: &BlueTsDebugInfo,
) -> Vec<DirectRootSymbolSlot> {
    let body = match program {
        bluejs::BlueJsProgramV1::Script(program) => &program.body,
        bluejs::BlueJsProgramV1::Module(module) => &module.body,
    };
    let slots = compiled.bytecode().root_declaration_binding_slots();
    if body.len() != provenance.len()
        || body.len() != slots.len()
        || static_info.compiler_options_hash != compiler_options_fingerprint
        || static_info.language_version != LANGUAGE_VERSION
        || !debug_attachment::source_sets_match(&static_info.sources, sources)
    {
        return Vec::new();
    }
    let Some(code_unit) = compiled.code_units().first().map(|unit| unit.id()) else {
        return Vec::new();
    };
    let mut slot_counts = BTreeMap::<u32, usize>::new();
    for slot in slots.iter().flatten() {
        *slot_counts.entry(*slot).or_default() += 1;
    }
    let mut symbols_by_span = BTreeMap::<(&str, usize, usize), Vec<_>>::new();
    let mut symbol_id_counts = BTreeMap::<SymbolId, usize>::new();
    for symbol in &static_info.symbols {
        symbols_by_span
            .entry((
                symbol.span.module.as_str(),
                symbol.span.start,
                symbol.span.end,
            ))
            .or_default()
            .push(symbol);
        *symbol_id_counts.entry(symbol.id).or_default() += 1;
    }
    let mut source_records = BTreeMap::<SourceId, Vec<_>>::new();
    for source in &static_info.sources {
        source_records.entry(source.id).or_default().push(source);
    }
    let mut type_counts = BTreeMap::<TypeId, usize>::new();
    for static_type in &static_info.types {
        *type_counts.entry(static_type.id).or_default() += 1;
    }
    let mut bridge_sources = BTreeMap::<&str, Vec<_>>::new();
    for source in sources {
        bridge_sources
            .entry(source.module.as_str())
            .or_default()
            .push(source);
    }

    body.iter()
        .zip(provenance)
        .zip(slots)
        .filter_map(|((statement, provenance), slot)| {
            let slot = (*slot)?;
            if slot_counts.get(&slot) != Some(&1)
                || provenance.kind != LoweringProvenanceKind::LoweredSyntax
            {
                return None;
            }
            let (name, kind) = match statement {
                bluejs::Stmt::VarDecl(_, declarations) => match declarations.as_slice() {
                    [declaration] => match &declaration.pattern {
                        bluejs::Pattern::Identifier(name) => (name.as_str(), SymbolKind::Variable),
                        _ => return None,
                    },
                    _ => return None,
                },
                bluejs::Stmt::FunctionDecl(function) => {
                    (function.name.as_deref()?, SymbolKind::Function)
                }
                _ => return None,
            };
            let [symbol] = symbols_by_span
                .get(&(
                    provenance.source.module.as_str(),
                    provenance.source.start,
                    provenance.source.end,
                ))?
                .as_slice()
            else {
                return None;
            };
            if symbol.name != name
                || symbol.kind != kind
                || symbol.location != provenance.location
                || symbol_id_counts.get(&symbol.id) != Some(&1)
            {
                return None;
            }
            let type_id = symbol.static_type?;
            if type_counts.get(&type_id) != Some(&1) {
                return None;
            }
            let [source] = source_records.get(&symbol.source)?.as_slice() else {
                return None;
            };
            let [bridge_source] = bridge_sources.get(source.module.as_str())?.as_slice() else {
                return None;
            };
            if source.module != provenance.source.module
                || source.content_hash != bridge_source.content_hash
            {
                return None;
            }
            Some(DirectRootSymbolSlot {
                program: handle,
                code_unit,
                slot_ordinal: slot,
                source_id: symbol.source,
                symbol_id: symbol.id,
                type_id,
            })
        })
        .collect()
}

impl BlueTsSafePointMapV1 {
    /// Resolves only an instruction inside a compiler-recorded owning root
    /// statement or direct child function to its original BlueTS byte span.
    /// Other instructions stay unbound; no nearest-statement or generated-
    /// source guess is made. Callers must validate the live program first.
    pub fn source_span_for_safe_point(
        &self,
        code_unit_ordinal: u32,
        bytecode_offset: u32,
    ) -> Option<&BlueTsSafePointEntryV1> {
        self.entries
            .binary_search_by_key(&(code_unit_ordinal, bytecode_offset), |entry| {
                (entry.code_unit.ordinal(), entry.bytecode_offset)
            })
            .ok()
            .map(|index| &self.entries[index])
    }

    /// Finds the nearest bound entry at or after one TypeScript UTF-8 byte
    /// position. This bound-only view is useful when a caller has retained the
    /// map independently; [`DirectProgramAttachment::breakpoint_at_or_after`]
    /// additionally preserves an explicitly unbound lowering span.
    pub fn nearest_bound_safe_point_at_or_after(
        &self,
        source: &str,
        source_byte: usize,
    ) -> Option<bluejs::BlueJsSafePoint> {
        self.entries
            .iter()
            .filter(|entry| entry.source == source && entry.end_byte > source_byte)
            .min_by_key(|entry| {
                (
                    entry.start_byte.saturating_sub(source_byte),
                    entry.start_byte,
                    entry.end_byte,
                )
            })
            .map(|entry| bluejs::BlueJsSafePoint {
                code_unit: entry.code_unit,
                bytecode_offset: entry.bytecode_offset,
            })
    }

    /// Verifies this map against its exact live BlueJS generation. The caller
    /// must separately compare the retained compiler/source fingerprints with
    /// its authorized page-load request before exposing the map.
    pub fn validate_against(
        &self,
        registry: &bluejs::BlueJsProgramRegistry,
        handle: bluejs::BlueJsProgramHandle,
    ) -> Result<(), BridgeError> {
        if self.format != BLUEJS_SAFE_POINT_MAP_ABI_V1
            || self.program_abi != bluejs::BLUEJS_PROGRAM_ABI_V1
            || self.program_generation != handle.generation().as_u64()
        {
            return Err(BridgeError::ProvenanceAttachment(
                "safe-point map ABI or generation does not match the live program".to_string(),
            ));
        }
        for entry in &self.entries {
            registry
                .validate_safe_point(
                    handle,
                    bluejs::BlueJsSafePoint {
                        code_unit: entry.code_unit,
                        bytecode_offset: entry.bytecode_offset,
                    },
                )
                .map_err(BridgeError::BlueJsDebug)?;
        }
        if !safe_point_entries_are_strictly_valid(&self.entries) {
            return Err(BridgeError::ProvenanceAttachment(
                "safe-point map entries are not sorted and unique".to_string(),
            ));
        }
        Ok(())
    }
}

pub(super) fn build_safe_point_map(
    handle: bluejs::BlueJsProgramHandle,
    compiler_options_fingerprint: &str,
    sources: &[BridgeSource],
    provenance: &[AttachedLoweringProvenance],
    compiled: &bluejs::BlueJsCompiledProgram,
) -> Result<BlueTsSafePointMapV1, BridgeError> {
    let bytecode = compiled.bytecode();
    let ranges = bytecode.root_statement_ranges();
    let child_indices = bytecode.root_function_child_indices();
    if ranges.len() != provenance.len() || child_indices.len() != provenance.len() {
        return Err(BridgeError::ProvenanceAttachment(
            "root statement ownership does not match direct lowering spans".to_string(),
        ));
    }
    let code_units = compiled.code_units();
    let root = code_units.first().ok_or_else(|| {
        BridgeError::ProvenanceAttachment("the installed program has no root code unit".to_string())
    })?;
    let children = bytecode.child_code_units().collect::<Vec<_>>();
    if children
        .iter()
        .any(|child| child.child_code_units().next().is_some())
        || code_units.len() != children.len() + 1
    {
        return Err(BridgeError::ProvenanceAttachment(
            "direct lowering produced an unsupported nested closure shape".to_string(),
        ));
    }
    let mut entries = Vec::new();
    for ((provenance, range), child_index) in provenance.iter().zip(ranges).zip(child_indices) {
        match (provenance.safe_point, range) {
            (DirectSafePointBinding::Bound(safe_point), Some((start, end)))
                if safe_point.code_unit == root.id()
                    && safe_point.bytecode_offset == *start
                    && start < end => {}
            (DirectSafePointBinding::Unbound, None) if child_index.is_none() => continue,
            _ => {
                return Err(BridgeError::ProvenanceAttachment(
                    "compiler statement range disagrees with its bound safe point".to_string(),
                ));
            }
        }
        let entry_at = |code_unit, bytecode_offset| BlueTsSafePointEntryV1 {
            code_unit,
            bytecode_offset,
            source: provenance.source.module.clone(),
            start_byte: provenance.source.start,
            end_byte: provenance.source.end,
            location: provenance.location,
            provenance_kind: provenance.kind,
        };
        let (start, end) = range.expect("bound statement has a range");
        entries.extend(
            root.instruction_offsets()
                .iter()
                .copied()
                .filter(|offset| *offset >= start && *offset < end)
                .map(|offset| entry_at(root.id(), offset)),
        );
        if let Some(child_index) = child_index {
            let child_ordinal = usize::try_from(*child_index)
                .ok()
                .and_then(|index| index.checked_add(1))
                .ok_or_else(|| {
                    BridgeError::ProvenanceAttachment(
                        "function child index exceeds the host address space".to_string(),
                    )
                })?;
            let child = code_units.get(child_ordinal).ok_or_else(|| {
                BridgeError::ProvenanceAttachment(
                    "function declaration has no installed child code unit".to_string(),
                )
            })?;
            entries.extend(
                child
                    .instruction_offsets()
                    .iter()
                    .copied()
                    .map(|offset| entry_at(child.id(), offset)),
            );
        }
    }
    entries.sort_by(safe_point_entry_order);
    if !safe_point_entries_are_strictly_valid(&entries) {
        return Err(BridgeError::ProvenanceAttachment(
            "multiple lowering spans resolved to the same safe-point map entry".to_string(),
        ));
    }
    Ok(BlueTsSafePointMapV1 {
        format: BLUEJS_SAFE_POINT_MAP_ABI_V1,
        program_abi: bluejs::BLUEJS_PROGRAM_ABI_V1,
        program_generation: handle.generation().as_u64(),
        compiler_options_fingerprint: compiler_options_fingerprint.to_string(),
        source_set_hash: source_set_hash(sources),
        entries,
    })
}

fn safe_point_entries_are_strictly_valid(entries: &[BlueTsSafePointEntryV1]) -> bool {
    entries.iter().all(|entry| {
        let start = entry.location.start;
        let end = entry.location.end;
        !entry.source.is_empty()
            && entry.start_byte < entry.end_byte
            && (start.line, start.column_utf16) < (end.line, end.column_utf16)
            && start
                .line
                .checked_add(start.column_utf16)
                .is_some_and(|position| position <= entry.start_byte)
            && end
                .line
                .checked_add(end.column_utf16)
                .is_some_and(|position| position <= entry.end_byte)
    }) && entries.windows(2).all(|pair| {
        safe_point_entry_order(&pair[0], &pair[1]).is_lt()
            && (pair[0].code_unit != pair[1].code_unit
                || pair[0].bytecode_offset != pair[1].bytecode_offset)
    })
}

fn safe_point_entry_order(
    left: &BlueTsSafePointEntryV1,
    right: &BlueTsSafePointEntryV1,
) -> std::cmp::Ordering {
    (
        left.code_unit.generation(),
        left.code_unit.ordinal(),
        left.bytecode_offset,
        &left.source,
        left.start_byte,
        left.end_byte,
        left.provenance_kind,
    )
        .cmp(&(
            right.code_unit.generation(),
            right.code_unit.ordinal(),
            right.bytecode_offset,
            &right.source,
            right.start_byte,
            right.end_byte,
            right.provenance_kind,
        ))
}

fn source_set_hash(sources: &[BridgeSource]) -> String {
    let mut source_identities = sources
        .iter()
        .map(|source| (&source.module, &source.content_hash))
        .collect::<Vec<_>>();
    source_identities.sort_unstable();
    let mut hash = 0xcbf29ce484222325u64;
    for (module, content_hash) in source_identities {
        for byte in module
            .bytes()
            .chain(std::iter::once(0xff))
            .chain(content_hash.bytes())
            .chain(std::iter::once(0xfe))
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("bts-source-set-{hash:016x}")
}
