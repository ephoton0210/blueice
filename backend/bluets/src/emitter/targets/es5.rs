// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ES5 lexical bindings and arrows, using retained parser and checker identities.

use super::*;
use crate::parser::{NestedFunctionBody, NestedFunctionKind};

pub(in crate::emitter) fn prepare(
    module: &Module,
    project: &Project,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
) -> Result<crate::checker::LexicalEmission, Diagnostic> {
    if options.target != crate::EcmaTarget::Es5 {
        return Ok(crate::checker::LexicalEmission::default());
    }
    let lexical = crate::checker::lexical_emission(project, module);
    edits.extend(lexical.edits.iter().cloned());
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &module.source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let arrows = module
        .nested_functions
        .values()
        .filter(|function| function.kind == NestedFunctionKind::Arrow && !function.async_function)
        .collect::<Vec<_>>();
    let mut sequence = 0;
    parameters::declarations(module, &tokens, edits, &mut sequence)?;
    let captures = arrows
        .iter()
        .filter(|function| {
            tokens.iter().any(|token| {
                token.is("this")
                    && !class_receiver(module, token, function.span.start)
                    && token.start >= function.span.start
                    && token.end <= function.span.end
                    && !module.nested_functions.values().any(|nested| {
                        nested.kind != NestedFunctionKind::Arrow
                            && nested.span.start > function.span.start
                            && token.start >= nested.span.start
                            && token.end <= nested.span.end
                    })
            })
        })
        .map(|function| {
            (
                function.span.start,
                unused_name(&module.source, "this", &mut sequence),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for function in arrows {
        let start = tokens.partition_point(|token| token.start < function.span.start);
        let arrow = tokens[start..]
            .iter()
            .position(|token| token.is("=>"))
            .map(|index| start + index)
            .ok_or_else(|| unsupported(module, &function.span, "arrow head was not retained"))?;
        let body = tokens
            .get(arrow + 1)
            .ok_or_else(|| unsupported(module, &function.span, "arrow body was not retained"))?;
        let capture = captures.get(&function.span.start);
        let prefix =
            capture.map_or_else(String::new, |name| format!("(function ({name}) {{ return "));
        let parent = module
            .nested_functions
            .values()
            .filter(|parent| {
                parent.span.start < function.span.start && parent.span.end >= function.span.end
            })
            .min_by_key(|parent| parent.span.end - parent.span.start);
        let receiver = parent
            .filter(|parent| parent.kind == NestedFunctionKind::Arrow)
            .and_then(|parent| captures.get(&parent.span.start))
            .map_or("this", String::as_str);
        let suffix = if capture.is_some() {
            format!("; }})({receiver})")
        } else {
            String::new()
        };
        let single = !tokens[start].is("(") && function.type_parameters.is_empty();
        edits.push(TextEdit {
            start: function.span.start,
            end: function.span.start,
            replacement: format!("{prefix}function {}", if single { "(" } else { "" }),
        });
        let concise = matches!(function.body, NestedFunctionBody::Expression(_));
        let prologue =
            parameters::prologue(module, &tokens, &function.parameters, edits, &mut sequence)?;
        edits.push(TextEdit {
            start: tokens[arrow].start,
            end: tokens[arrow].end,
            replacement: format!(
                "{}{}",
                if single { ")" } else { "" },
                if concise {
                    format!(" {{ {prologue}return (")
                } else {
                    " ".into()
                },
            ),
        });
        if !concise && !prologue.is_empty() {
            let insertion = parameters::body_directive_end(&tokens, body.start);
            edits.push(TextEdit {
                start: insertion,
                end: insertion,
                replacement: format!("\n{prologue}"),
            });
        }
        edits.push(TextEdit {
            start: function.span.end,
            end: function.span.end,
            replacement: format!("{}{suffix}", if concise { "); }" } else { "" }),
        });
        if let Some(capture) = capture {
            for token in tokens.iter().filter(|token| {
                token.is("this")
                    && !class_receiver(module, token, function.span.start)
                    && token.start >= body.start
                    && token.end <= function.span.end
                    && !module.nested_functions.values().any(|nested| {
                        nested.span.start > function.span.start
                            && token.start >= nested.span.start
                            && token.end <= nested.span.end
                    })
            }) {
                edits.push(TextEdit {
                    start: token.start,
                    end: token.end,
                    replacement: capture.clone(),
                });
            }
        }
    }
    Ok(lexical)
}

/// A class body introduces its own receiver; computed keys keep the outer one.
fn class_receiver(module: &Module, token: &Token, arrow_start: usize) -> bool {
    runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| match declaration {
            Declaration::Class(class) => Some(class),
            _ => None,
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| &expression.class),
        )
        .any(|class| {
            class.span.start > arrow_start
                && token.start >= class.body_span.start
                && token.end <= class.body_span.end
                && !class.members.iter().any(|member| {
                    member.key.first().is_some_and(|key| key.is("["))
                        && member
                            .key
                            .first()
                            .is_some_and(|key| token.start >= key.start)
                        && member.key.last().is_some_and(|key| token.end <= key.end)
                })
        })
}

pub(in crate::emitter) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
    lexical: &crate::checker::LexicalEmission,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target != crate::EcmaTarget::Es5 {
        return Ok(emitted);
    }
    emitted = templates::lower(emitted, module, options)?;
    emitted = patterns::lower(emitted, module, options)?;
    emitted = objects::lower(emitted, module, options)?;
    emitted = classes::lower(emitted, module, options)?;
    emitted = spreads::lower(emitted, module, options)?;
    emitted = iteration::lower(emitted, module, options)?;
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &emitted.javascript,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let edits = tokens
        .iter()
        .filter(|token| matches!(token.text.as_str(), "let" | "const"))
        .map(|token| TextEdit {
            start: token.start,
            end: token.end,
            replacement: "var".into(),
        })
        .collect();
    emitted = mapped_edits(emitted, edits);
    emitted = loops::lower(emitted, module, options, &lexical.captured_loop_bindings)?;
    suspension::lower(emitted, module, options)
}

fn unsupported(module: &Module, span: &SourceSpan, message: &str) -> Diagnostic {
    Diagnostic::error(
        DiagnosticCode::UnsupportedSyntax,
        SourceSpan::new(&module.id, span.start, span.end),
        format!("ES5 lowering: {message}"),
    )
}

mod classes;
mod iteration;
mod loops;
mod objects;
mod parameters;
mod patterns;
mod spreads;
mod suspension;
mod templates;
