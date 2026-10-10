// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Target-specific transforms after TypeScript erasure and module rewriting.

use super::*;
use crate::syntax::{Token, TokenKind};

mod async_functions;
mod async_generators;
mod async_iteration;
pub(super) mod es5;
mod exponentiation;
mod object_rest;
mod object_spread;
mod optional;

pub const TARGET_HELPER_V1_VERSION: &str = "blue-ts-target-helper-v1";
pub(crate) const TARGET_HELPER_V1_SOURCE: &str = include_str!("targets/helpers.v1.js");
pub(crate) const TARGET_HELPER_V1_SOURCES: &[&str] = &[
    TARGET_HELPER_V1_SOURCE,
    include_str!("targets/optional_helpers.v1.js"),
    include_str!("targets/object_spread_helpers.v1.js"),
    include_str!("targets/async_helpers.v1.js"),
    include_str!("targets/async_iteration_helpers.v1.js"),
    include_str!("targets/async_generator_helpers.v1.js"),
    include_str!("targets/es5_helpers.v1.js"),
    include_str!("targets/iteration_helpers.v1.js"),
    include_str!("targets/computed_property_helpers.v1.js"),
    include_str!("targets/spread_helpers.v1.js"),
    include_str!("targets/class_helpers.v1.js"),
    include_str!("targets/generator_helpers.v1.js"),
];

/// Lower binding logical assignments without moving their right operand
/// into another function or changing its suspension and lexical environment.
pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    emitted = exponentiation::lower(emitted, module, options)?;
    emitted = optional::lower(emitted, module, options)?;
    emitted = object_spread::lower(emitted, module, options)?;
    emitted = object_rest::lower(emitted, module, options)?;
    emitted = async_iteration::lower(emitted, module, options)?;
    emitted = async_generators::lower(emitted, module, options)?;
    emitted = async_functions::lower(emitted, module, options)?;
    if options.target >= crate::EcmaTarget::Es2021 {
        return Ok(emitted);
    }
    let mut remaining_tokens = options.limits.parser.max_tokens;
    let mut sequence = 0;
    loop {
        if !["||=", "&&=", "??="]
            .iter()
            .any(|operator| emitted.javascript.contains(operator))
        {
            return Ok(emitted);
        }
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining_tokens,
        )
        .map_err(|mut diagnostics| diagnostics.remove(0))?;
        remaining_tokens = remaining_tokens.saturating_sub(tokens.len());
        let candidate = tokens.iter().enumerate().rev().find(|(index, token)| {
            matches!(token.text.as_str(), "||=" | "&&=" | "??=")
                && index.checked_sub(1).is_some_and(|left| {
                    tokens[left].kind == TokenKind::Identifier
                        && !left.checked_sub(1).is_some_and(|before| {
                            tokens[before].is(".")
                                || tokens[before].is("?.")
                                || tokens[before].is("#")
                        })
                })
        });
        let Some((operator, token)) = candidate else {
            return Ok(emitted);
        };
        let left = &tokens[operator - 1];
        let end = assignment_end(&tokens, operator + 1);
        let Some(last) = tokens
            .get(end.saturating_sub(1))
            .filter(|_| end > operator + 1)
        else {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                SourceSpan::new(&module.id, 0, 0),
                "logical assignment has no right operand",
            ));
        };
        let right = &emitted.javascript[token.end..last.end];
        let mut edits = Vec::new();
        let replacement = match token.text.as_str() {
            "||=" => format!("({0} || ({0} = {right}))", left.text),
            "&&=" => format!("({0} && ({0} = {right}))", left.text),
            _ => {
                let temporary = unused_name(&emitted.javascript, "value", &mut sequence);
                let helper = unused_name(&emitted.javascript, "is_nullish", &mut sequence);
                let insertion = directive_end(&tokens);
                edits.push(TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!(
                        "\nvar {temporary};\n{}\n",
                        TARGET_HELPER_V1_SOURCE.replace("__blueice_target_is_nullish", &helper)
                    ),
                });
                format!(
                    "({temporary} = {0}, {helper}({temporary}) ? ({0} = {right}) : {temporary})",
                    left.text
                )
            }
        };
        edits.push(TextEdit {
            start: left.start,
            end: last.end,
            replacement,
        });
        emitted = mapped_edits(emitted, edits);
    }
}

fn unused_name(source: &str, kind: &str, sequence: &mut usize) -> String {
    loop {
        let name = format!("__blueice_target_{kind}_{sequence}");
        *sequence += 1;
        if !source.contains(&name) {
            return name;
        }
    }
}

fn directive_end(tokens: &[Token]) -> usize {
    let mut end = 0;
    let mut index = 0;
    while tokens
        .get(index)
        .is_some_and(|token| token.kind == TokenKind::String)
        && tokens.get(index + 1).is_some_and(|token| token.is(";"))
    {
        end = tokens[index + 1].end;
        index += 2;
    }
    end
}

fn assignment_end(tokens: &[Token], start: usize) -> usize {
    let mut nesting = 0;
    let mut conditional = 0;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "(" | "[" | "{" => nesting += 1,
            ")" | "]" | "}" if nesting == 0 => return index,
            ")" | "]" | "}" => nesting -= 1,
            "?" if nesting == 0 => conditional += 1,
            ":" if nesting == 0 && conditional == 0 => return index,
            ":" if nesting == 0 => conditional -= 1,
            ";" | "," if nesting == 0 => return index,
            _ if token.kind == TokenKind::Eof => return index,
            _ => {}
        }
    }
    tokens.len()
}

fn mapped_edits(previous: EmittedJavaScript, edits: Vec<TextEdit>) -> EmittedJavaScript {
    let mut next = apply_edits(&previous.javascript, edits);
    for segment in &mut next.provenance {
        let key = (segment.source_line, segment.source_column);
        let index = previous
            .provenance
            .partition_point(|mapping| (mapping.generated_line, mapping.generated_column) <= key);
        if let Some(mapping) = previous.provenance.get(index.saturating_sub(1)) {
            let column = if mapping.generated_line == segment.source_line {
                segment
                    .source_column
                    .saturating_sub(mapping.generated_column)
            } else {
                0
            };
            segment.source_line = mapping.source_line;
            segment.source_column = mapping.source_column.saturating_add(column);
        }
    }
    next
}
