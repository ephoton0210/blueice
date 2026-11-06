// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A code-generation arrow receives source-callable roles only with exact AST identity.
use super::*;
use crate::parser::NestedFunction;
use std::collections::BTreeSet;

type Position = (usize, usize);

pub(super) enum Phase<'a> {
    Source,
    Generated {
        previous: &'a EmittedJavaScript,
        source_ranges: BTreeSet<(Position, Position)>,
    },
}

fn position(source: &str, offset: usize) -> Position {
    let prefix = &source[..offset];
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count(),
        prefix.rsplit('\n').next().unwrap().encode_utf16().count(),
    )
}

impl<'a> Phase<'a> {
    pub(super) fn generated(original: &Module, previous: &'a EmittedJavaScript) -> Self {
        Self::Generated {
            previous,
            source_ranges: original
                .nested_functions
                .values()
                .filter(|function| {
                    function.kind == NestedFunctionKind::Arrow && !function.async_function
                })
                .map(|function| {
                    (
                        position(&original.source, function.span.start),
                        position(&original.source, function.span.end),
                    )
                })
                .collect(),
        }
    }

    pub(super) fn is_source(&self, module: &Module, function: &NestedFunction) -> bool {
        match self {
            Self::Source => true,
            Self::Generated {
                previous,
                source_ranges,
            } => {
                let origin = |offset| {
                    let key = position(&module.source, offset);
                    let index = previous.provenance.partition_point(|point| {
                        (point.generated_line, point.generated_column) <= key
                    });
                    index.checked_sub(1).and_then(|index| {
                        previous
                            .provenance
                            .get(index)
                            .filter(|point| point.mapped && point.generated_line == key.0)
                            .map(|point| (point.source_line, point.source_column))
                    })
                };
                match (origin(function.span.start), origin(function.span.end)) {
                    (Some(start), Some(end)) => source_ranges.contains(&(start, end)),
                    _ => false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_phase_requires_both_exact_original_arrow_endpoints() {
        for prefix in ["", "/* 🧭 */\r\n"] {
            let original = crate::parser::parse_module(
                "memory:///main.ts",
                format!("{prefix}const source=(value:number)=>value+1;"),
            )
            .unwrap();
            let source = original.nested_functions.values().next().unwrap();
            let mut edits = original.edits.clone();
            edits.push(TextEdit {
                start: original.source.len(),
                end: original.source.len(),
                replacement: format!(
                    "{} const generated=(value)=>value+1;{}",
                    crate::emitter::relocations::source_anchor(source.span.start),
                    crate::emitter::relocations::unmapped()
                ),
            });
            let previous = crate::emitter::apply_edits_with_anchors(
                &original.source,
                edits,
                Some(10_000),
                BTreeMap::new(),
                BTreeMap::new(),
                None,
            );
            let owned = crate::parser::parse_emitted_module(
                &original.id,
                &previous.javascript,
                &CompilerOptions::default().limits.parser,
            )
            .unwrap();
            let arrows = owned.nested_functions.values().collect::<Vec<_>>();
            assert_eq!(arrows.len(), 2);
            let phase = Phase::generated(&original, &previous);
            assert!(phase.is_source(&owned, arrows[0]));
            assert!(!phase.is_source(&owned, arrows[1]));
        }
    }
}
