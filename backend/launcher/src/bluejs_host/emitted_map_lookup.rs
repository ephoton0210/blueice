// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exact child-private lookup in a live compiler-emitted JavaScript map.

use super::*;

impl BoundEmittedMap {
    /// Certify the one compiler-emitted ES5 object-spread shape whose two
    /// same-named callees have a provable evaluation order: the outer call's
    /// callee is evaluated before the inner first-argument call's callee.
    /// This is checked from the parser AST, not inferred from nearby tokens.
    pub(super) fn certify_nested_spread_helper(&self) -> Option<CertifiedNestedSpreadHelper> {
        if !self
            .generated
            .source
            .contains("__blueice_target_object_properties_")
        {
            return None;
        }
        let (program, statements, identifiers) =
            blueice_bluejs::parse_with_debug_spans(&self.generated.source).ok()?;
        if statements != self.top_level_spans || identifiers != self.identifier_spans {
            return None;
        }
        let mut candidates = program.body.iter().enumerate().filter_map(|(index, statement)| {
            let blueice_bluejs::Stmt::VarDecl(_, declarations) = statement else {
                return None;
            };
            let [declaration] = declarations.as_slice() else {
                return None;
            };
            if !matches!(declaration.pattern, blueice_bluejs::Pattern::Identifier(_)) {
                return None;
            }
            let Some(blueice_bluejs::Expr::Call { callee, args }) = &declaration.init else {
                return None;
            };
            let blueice_bluejs::Expr::Identifier(name) = callee.as_ref() else {
                return None;
            };
            if !name.starts_with("__blueice_target_object_properties_") {
                return None;
            }
            let Some(blueice_bluejs::Argument::Normal(blueice_bluejs::Expr::Call {
                callee: inner_callee,
                ..
            })) = args.first()
            else {
                return None;
            };
            if !matches!(inner_callee.as_ref(), blueice_bluejs::Expr::Identifier(inner) if inner == name)
            {
                return None;
            }
            let statement_span = self.top_level_spans.get(index)?;
            let mut same_name = self.identifier_spans.iter().filter(|span| {
                span.name == *name
                    && statement_span.start_byte <= span.start_byte
                    && span.end_byte <= statement_span.end_byte
                    && self.generated.source.get(span.start_byte..span.end_byte)
                        == Some(name.as_str())
            });
            let outer = same_name.next()?.start_byte;
            let inner = same_name.next()?.start_byte;
            (same_name.next().is_none() && outer < inner).then(|| CertifiedNestedSpreadHelper {
                statement_index: index,
                name: name.clone(),
                source_starts: [outer, inner],
            })
        });
        let candidate = candidates.next()?;
        candidates.next().is_none().then_some(candidate)
    }

    fn exact_nested_spread_helper_source_start(
        &self,
        installed: &blueice_bluejs::BlueJsCompiledProgram,
        point: blueice_bluejs::BlueJsSafePoint,
    ) -> Option<usize> {
        let helper = self.nested_spread_helper.as_ref()?;
        let points =
            installed.two_root_identifier_safe_points(helper.statement_index, &helper.name)?;
        points
            .iter()
            .position(|candidate| *candidate == point)
            .map(|index| helper.source_starts[index])
    }

    /// A parser-produced identifier token is eligible only when its spelling
    /// is literal and unique within the owning root statement. Escaped names,
    /// duplicate names, and nested function-only identifiers stay unbound.
    fn unique_identifier_source_start(&self, statement_index: usize, name: &str) -> Option<usize> {
        let statement = self.top_level_spans.get(statement_index)?;
        let mut matches = self.identifier_spans.iter().filter(|span| {
            span.name == name
                && statement.start_byte <= span.start_byte
                && span.end_byte <= statement.end_byte
        });
        let span = matches.next()?;
        if matches.next().is_some()
            || self.generated.source.get(span.start_byte..span.end_byte) != Some(name)
        {
            return None;
        }
        Some(span.start_byte)
    }

    fn exact_identifier_safe_point(
        &self,
        installed: &blueice_bluejs::BlueJsCompiledProgram,
        byte: usize,
    ) -> Option<blueice_bluejs::BlueJsSafePoint> {
        let (statement_index, _) = self
            .top_level_spans
            .iter()
            .enumerate()
            .find(|(_, span)| span.start_byte <= byte && byte < span.end_byte)?;
        let mut matches = self
            .identifier_spans
            .iter()
            .filter(|span| span.start_byte == byte);
        let identifier = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let point = if self.unique_identifier_source_start(statement_index, &identifier.name)
            == Some(byte)
        {
            installed.unique_identifier_safe_point(statement_index, &identifier.name)?
        } else {
            let helper = self.nested_spread_helper.as_ref()?;
            if helper.statement_index != statement_index || helper.name != identifier.name {
                return None;
            }
            let index = helper
                .source_starts
                .iter()
                .position(|start| *start == byte)?;
            installed.two_root_identifier_safe_points(statement_index, &identifier.name)?[index]
        };
        if point.code_unit.ordinal() == 0
            && self
                .non_evaluating_top_level_statements
                .contains(&statement_index)
        {
            return None;
        }
        (installed.code_units()[point.code_unit.ordinal() as usize].nesting_depth() <= 2)
            .then_some(point)
    }
}

impl BlueJsChildHost {
    fn exact_emitted_map(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> Result<(&BoundEmittedMap, &blueice_bluejs::BlueJsCompiledProgram), PageHostReply> {
        if !program.is_well_formed() {
            return Err(invalid_request());
        }
        let document = self.exact_document(tab_id, document_generation)?;
        let Some(record) = document.debugger_programs.get(&program.program_handle) else {
            return Err(invalid_request());
        };
        if record.program_generation != program.program_generation {
            return Err(invalid_request());
        }
        let Some(bound) = document.emitted_maps.get(&program) else {
            return Err(invalid_request());
        };
        let Ok(installed) = self.runtime.program_registry().get(record.runtime_handle) else {
            return Err(invalid_request());
        };
        if installed.source().canonical_module_id() != bound.generated.canonical_module_id
            || installed.source().source_hash() != bound.generated.source_hash
        {
            return Err(invalid_request());
        }
        Ok((bound, installed))
    }

    pub(super) fn debugger_emitted_original_position(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        generated_line: u32,
        generated_column_utf16: u32,
    ) -> PageHostReply {
        let (bound, _) = match self.exact_emitted_map(tab_id, document_generation, program) {
            Ok(bound) => bound,
            Err(reply) => return reply,
        };
        let original = match bound.map.original_position_at(
            &bound.generated.source,
            generated_line as usize,
            generated_column_utf16 as usize,
            BoundEmittedMap::MAX_SEGMENTS,
            BoundEmittedMap::MAX_RETAINED_BYTES,
        ) {
            Ok(original) => original,
            Err(blueice_bluets::SourceMapLookupError::QueryOutOfBounds) => {
                return invalid_request();
            }
            Err(_) => return host_failure(),
        };
        let original = match original.map(|original| {
            Some(PageHostDebuggerEmittedOriginalPosition {
                source_index: u32::try_from(original.source_index).ok()?,
                line: u32::try_from(original.line).ok()?,
                column_utf16: u32::try_from(original.column_utf16).ok()?,
            })
        }) {
            Some(Some(original)) => Some(original),
            Some(None) => return host_failure(),
            None => None,
        };
        PageHostReply::DebuggerEmittedOriginalPosition {
            tab_id,
            document_generation,
            program,
            generated_line,
            generated_column_utf16,
            original,
        }
    }

    pub(super) fn debugger_emitted_safe_point(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
        generated_line: u32,
        generated_column_utf16: u32,
    ) -> PageHostReply {
        let (bound, installed) = match self.exact_emitted_map(tab_id, document_generation, program)
        {
            Ok(bound) => bound,
            Err(reply) => return reply,
        };
        let original = match bound.map.original_position_at(
            &bound.generated.source,
            generated_line as usize,
            generated_column_utf16 as usize,
            BoundEmittedMap::MAX_SEGMENTS,
            BoundEmittedMap::MAX_RETAINED_BYTES,
        ) {
            Ok(original) => original,
            Err(blueice_bluets::SourceMapLookupError::QueryOutOfBounds) => {
                return invalid_request();
            }
            Err(_) => return host_failure(),
        };
        if installed
            .ast_nodes()
            .iter()
            .filter(|node| node.is_top_level_statement())
            .count()
            != bound.top_level_spans.len()
        {
            return host_failure();
        }
        let safe_point = original
            .and_then(|_| {
                generated_utf16_byte(
                    &bound.generated.source,
                    generated_line,
                    generated_column_utf16,
                )
            })
            .and_then(|byte| {
                let statement = bound
                    .top_level_spans
                    .iter()
                    .position(|span| span.start_byte == byte)
                    .and_then(|index| {
                        if bound.non_evaluating_top_level_statements.contains(&index) {
                            return None;
                        }
                        installed
                            .ast_nodes()
                            .iter()
                            .filter(|node| node.is_top_level_statement())
                            .nth(index)
                            .and_then(|node| installed.safe_point_for_ast_node(node.id()).ok())
                    });
                statement.or_else(|| bound.exact_identifier_safe_point(installed, byte))
            })
            .map(|point| PageHostDebuggerSafePoint {
                program,
                code_unit_ordinal: point.code_unit.ordinal(),
                bytecode_offset: point.bytecode_offset,
            })
            .filter(|point| {
                // A System module's execute body is entered through a page-supplied
                // System.register loader, possibly from another installed program.
                // The bounded nested scheduler cannot retain that foreign caller
                // chain, so even an exact token/bytecode match must fail closed.
                point.code_unit_ordinal == 0
                    || bound.emission_profile != PageHostBlueTsEmissionProfile::System
            });
        if let Some(safe_point) = safe_point {
            if let Err(reply) =
                self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
            {
                return reply;
            }
        }
        PageHostReply::DebuggerEmittedSafePoint {
            tab_id,
            document_generation,
            program,
            generated_line,
            generated_column_utf16,
            safe_point,
        }
    }

    pub(super) fn debugger_emitted_paused_root_location(
        &self,
        tab_id: u64,
        document_generation: u64,
        program: PageHostDebuggerProgram,
    ) -> PageHostReply {
        let (bound, installed) = match self.exact_emitted_map(tab_id, document_generation, program)
        {
            Ok(bound) => bound,
            Err(reply) => return reply,
        };
        let document = match self.exact_document(tab_id, document_generation) {
            Ok(document) => document,
            Err(reply) => return reply,
        };
        if !document.debugger_execution_control {
            return invalid_debugger_state();
        }
        let pending_entry = document
            .pending_debugger_executions
            .front()
            .and_then(|pending| pending.program);
        let safe_point = match document.debugger_execution_states.get(&program) {
            Some(ChildDebuggerExecutionStatus::Paused(safe_point))
            | Some(ChildDebuggerExecutionStatus::NestedPaused { safe_point, .. }) => {
                let runtime_frame = match document.debugger_execution_states.get(&program) {
                    Some(ChildDebuggerExecutionStatus::NestedPaused { frame, .. }) => Some(*frame),
                    _ => None,
                };
                if pending_entry != Some(program)
                    || safe_point.program != program
                    || runtime_frame.is_some() == (safe_point.code_unit_ordinal == 0)
                    || runtime_frame.is_some_and(|frame| {
                        frame.tab_id() != tab_id
                            || frame.program() != installed.handle()
                            || frame.code_unit_ordinal() != safe_point.code_unit_ordinal
                    })
                {
                    return invalid_debugger_state();
                }
                let frame_count = if runtime_frame.is_some() { 3 } else { 1 };
                let stack = match self.runtime.debugger_stack_snapshot(
                    tab_id,
                    installed.handle(),
                    runtime_frame,
                    frame_count,
                    1,
                ) {
                    Ok(stack) => stack,
                    Err(_) => return invalid_debugger_state(),
                };
                if stack.stack_truncated
                    || runtime_frame.is_some() && !(2..=3).contains(&stack.frames.len())
                    || runtime_frame.is_none() && stack.frames.len() != 1
                    || stack.frames.first().is_none_or(|frame| {
                        frame.code_unit_ordinal != safe_point.code_unit_ordinal
                            || frame.bytecode_offset != safe_point.bytecode_offset
                    })
                    || runtime_frame.is_some()
                        && stack
                            .frames
                            .last()
                            .is_none_or(|frame| frame.code_unit_ordinal != 0)
                    || stack.frames.len() == 3 && stack.frames[1].code_unit_ordinal == 0
                {
                    return invalid_debugger_state();
                }
                *safe_point
            }
            _ => {
                let Some(entry) = pending_entry else {
                    return invalid_debugger_state();
                };
                let Some(ChildDebuggerExecutionStatus::LinkedPaused { frame, .. }) =
                    document.debugger_execution_states.get(&entry)
                else {
                    return invalid_debugger_state();
                };
                let stack = match self.debugger_linked_stack_snapshot(
                    tab_id,
                    document_generation,
                    entry,
                    *frame,
                    1,
                ) {
                    Ok(stack) if !stack.stack_truncated => stack,
                    _ => return invalid_debugger_state(),
                };
                let mut matching = stack
                    .frames
                    .iter()
                    .filter(|frame| frame.safe_point.program == program);
                let Some(selected) = matching.next() else {
                    return invalid_debugger_state();
                };
                if matching.next().is_some() {
                    return invalid_debugger_state();
                }
                selected.safe_point
            }
        };
        if let Err(reply) = self.exact_debugger_safe_point(tab_id, document_generation, safe_point)
        {
            return reply;
        }
        if installed
            .ast_nodes()
            .iter()
            .filter(|node| node.is_top_level_statement())
            .count()
            != bound.top_level_spans.len()
        {
            return host_failure();
        }
        let mut exact_starts = BTreeSet::new();
        for (index, (node, span)) in installed
            .ast_nodes()
            .iter()
            .filter(|node| node.is_top_level_statement())
            .zip(&bound.top_level_spans)
            .enumerate()
        {
            if bound.non_evaluating_top_level_statements.contains(&index) {
                continue;
            }
            let Ok(candidate) = installed.safe_point_for_ast_node(node.id()) else {
                continue;
            };
            if candidate.code_unit.ordinal() == safe_point.code_unit_ordinal
                && candidate.bytecode_offset == safe_point.bytecode_offset
            {
                exact_starts.insert(span.start_byte);
            }
        }
        let paused_point = blueice_bluejs::BlueJsSafePoint {
            code_unit: installed.code_units()[safe_point.code_unit_ordinal as usize].id(),
            bytecode_offset: safe_point.bytecode_offset,
        };
        let identifier_start = installed
            .unique_identifier_at_safe_point(paused_point)
            .filter(|_| {
                installed.code_units()[safe_point.code_unit_ordinal as usize].nesting_depth() <= 2
            })
            .and_then(|(statement_index, name)| {
                (safe_point.code_unit_ordinal != 0
                    || !bound
                        .non_evaluating_top_level_statements
                        .contains(&statement_index))
                .then(|| bound.unique_identifier_source_start(statement_index, name))
                .flatten()
            });
        // A compiler-recorded identifier evaluation names this exact
        // instruction more precisely than a coincident statement boundary.
        // Its source byte was independently certified as one literal parser
        // token; neither the statement start nor nearby tokens are guessed.
        // The separately certified ES5 outer/inner pair retains precedence
        // when the same-named callee occurs twice in one statement.
        let exact_start = bound
            .exact_nested_spread_helper_source_start(installed, paused_point)
            .or(identifier_start)
            .or_else(|| {
                (exact_starts.len() == 1)
                    .then(|| exact_starts.iter().next().copied())
                    .flatten()
            });
        let location = match exact_start {
            Some(start) => {
                let Some((generated_line, generated_column_utf16)) =
                    generated_byte_position(&bound.generated.source, start)
                else {
                    return host_failure();
                };
                let original = match bound.map.original_position_at(
                    &bound.generated.source,
                    generated_line as usize,
                    generated_column_utf16 as usize,
                    BoundEmittedMap::MAX_SEGMENTS,
                    BoundEmittedMap::MAX_RETAINED_BYTES,
                ) {
                    Ok(original) => original,
                    Err(_) => return host_failure(),
                };
                let original = match original.map(|position| {
                    Some(PageHostDebuggerEmittedOriginalPosition {
                        source_index: u32::try_from(position.source_index).ok()?,
                        line: u32::try_from(position.line).ok()?,
                        column_utf16: u32::try_from(position.column_utf16).ok()?,
                    })
                }) {
                    Some(Some(position)) => Some(position),
                    Some(None) => return host_failure(),
                    None => None,
                };
                Some(PageHostDebuggerEmittedPausedRootLocation {
                    generated_line,
                    generated_column_utf16,
                    original,
                })
            }
            None => None,
        };
        PageHostReply::DebuggerEmittedPausedRootLocation {
            tab_id,
            document_generation,
            program,
            safe_point,
            location,
        }
    }
}

/// Only exact UTF-16 character boundaries can name an AST token; the middle
/// of an astral character has no byte offset and must remain unbound.
fn generated_utf16_byte(text: &str, line: u32, column_utf16: u32) -> Option<usize> {
    let mut start = 0usize;
    for _ in 0..line {
        let newline = text.get(start..)?.find('\n')?;
        start = start.checked_add(newline + 1)?;
    }
    let rest = text.get(start..)?;
    let line_text = rest.split('\n').next()?;
    let mut column = 0usize;
    for (byte, character) in line_text.char_indices() {
        if column == column_utf16 as usize {
            return start.checked_add(byte);
        }
        column = column.checked_add(character.len_utf16())?;
        if column > column_utf16 as usize {
            return None;
        }
    }
    (column == column_utf16 as usize).then(|| start + line_text.len())
}

fn generated_byte_position(text: &str, byte: usize) -> Option<(u32, u32)> {
    let prefix = text.get(..byte)?;
    let line = u32::try_from(prefix.bytes().filter(|value| *value == b'\n').count()).ok()?;
    let column = u32::try_from(prefix.rsplit('\n').next()?.encode_utf16().count()).ok()?;
    Some((line, column))
}

#[cfg(test)]
mod tests {
    use super::{generated_byte_position, generated_utf16_byte, BoundEmittedMap};
    use blueice_bluejs::{BlueJsProgramRegistry, BlueJsProgramV1, BlueJsSourceIdentity};
    use blueice_bluets::{CompilerOptions, EcmaTarget, MapLoader, ModuleSource};
    use blueice_ipc::page_host::{PageHostBlueTsEmissionProfile, PageHostSource};

    #[test]
    fn generated_byte_lookup_requires_an_exact_utf16_boundary() {
        let text = "first\r\n🚀x\nlast";
        assert_eq!(generated_utf16_byte(text, 1, 0), Some(7));
        assert_eq!(generated_utf16_byte(text, 1, 1), None);
        assert_eq!(generated_utf16_byte(text, 1, 2), Some(11));
        assert_eq!(generated_utf16_byte(text, 1, 3), Some(12));
        assert_eq!(generated_utf16_byte(text, 2, 0), Some(13));
        assert_eq!(generated_utf16_byte(text, 3, 0), None);
        assert_eq!(generated_byte_position(text, 7), Some((1, 0)));
        assert_eq!(generated_byte_position(text, 11), Some((1, 2)));
        assert_eq!(generated_byte_position(text, 8), None);
    }

    #[test]
    fn certified_es5_nested_helper_binds_and_reverse_projects_both_callees() {
        let module = "memory:///nested-spread.ts";
        let source = "const base = { a: 1 }; const merged = { ...base, b: 2 };";
        let compilation = blueice_bluets::compile(
            module,
            &MapLoader::from([ModuleSource::new(module, source)]),
            CompilerOptions {
                target: EcmaTarget::Es5,
                source_map: true,
                inline_sources: true,
                ..Default::default()
            },
        );
        assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
        let artifact = &compilation.output.unwrap().artifacts[module];
        let generated = &artifact.javascript;
        let (parsed, statements, identifiers) =
            blueice_bluejs::parse_with_debug_spans(generated).unwrap();
        let bound = BoundEmittedMap::new(
            PageHostSource::new("memory:///nested-spread.js", generated),
            artifact.source_map.as_ref().unwrap().clone(),
            PageHostBlueTsEmissionProfile::Es5,
            statements,
            identifiers,
        )
        .unwrap();
        assert!(bound.nested_spread_helper.is_some());
        let mut registry = BlueJsProgramRegistry::default();
        let handle = registry
            .install(
                BlueJsSourceIdentity::new("memory:///nested-spread.js", "sha256:nested-spread")
                    .unwrap(),
                &BlueJsProgramV1::Script(parsed),
            )
            .unwrap();
        let installed = registry.get(handle).unwrap();
        let body = generated.find("var merged").unwrap();
        let helper = "__blueice_target_object_properties_";
        let outer = body + generated[body..].find(helper).unwrap();
        let inner = outer + 1 + generated[outer + 1..].find(helper).unwrap();
        let outer_point = bound.exact_identifier_safe_point(installed, outer).unwrap();
        let inner_point = bound.exact_identifier_safe_point(installed, inner).unwrap();
        assert_ne!(outer_point, inner_point);
        assert_eq!(
            bound.exact_nested_spread_helper_source_start(installed, outer_point),
            Some(outer)
        );
        assert_eq!(
            bound.exact_nested_spread_helper_source_start(installed, inner_point),
            Some(inner)
        );
        assert_eq!(registry.validate_safe_point(handle, outer_point), Ok(()));
        assert_eq!(registry.validate_safe_point(handle, inner_point), Ok(()));
    }
}
