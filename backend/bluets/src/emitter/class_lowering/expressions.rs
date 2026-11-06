// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Downlevel class expressions keep their stores and keys local to each evaluation.

use super::*;
use crate::parser::ClassExpression;

pub(in crate::emitter) mod names;
mod suspension;

pub(super) fn needs_wrapper(class: &ClassDeclaration) -> bool {
    class.members.iter().any(|member| {
        member.static_block.is_some()
            || member.field.as_ref().is_some_and(|field| {
                !field.declared
                    && (field.is_static
                        || field.name.starts_with('#')
                        || member.key.first().is_some_and(|token| token.is("[")))
            })
            || member
                .method
                .as_ref()
                .is_some_and(|method| method.name.starts_with('#'))
            || member
                .accessor
                .as_ref()
                .is_some_and(|accessor| accessor.name.starts_with('#'))
    })
}

pub(super) fn lower(
    module: &Module,
    expression: &ClassExpression,
    emit: FieldEmit,
    edits: &mut Vec<TextEdit>,
    auto_origins: &BTreeMap<usize, origins::auto_accessors::AutoField>,
    replacement: Option<&super::replacements::Replacement>,
) -> Result<(), Diagnostic> {
    let mut class = expression.class.clone();
    let tokens = crate::lex(&module.id, &module.source).map_err(|mut errors| errors.remove(0))?;
    class.name = expression.name.clone().unwrap_or_else(|| {
        let mut name = format!("_btsClass{}", class.span.start);
        while tokens.iter().any(|token| token.text == name) {
            name.push('_');
        }
        name
    });
    if expression.name.is_none() {
        let class_token = tokens
            .iter()
            .find(|token| token.start == class.span.start && token.is("class"))
            .expect("the owned expression starts at its class token");
        edits.push(TextEdit {
            start: class_token.end,
            end: class_token.end,
            replacement: if emit.standard_output && class.extends_span.is_some() {
                format!(
                    "{} {}{}{}",
                    crate::emitter::relocations::source_anchor(class_token.end),
                    crate::emitter::relocations::unmapped(),
                    class.name,
                    crate::emitter::relocations::source_anchor(class_token.end)
                )
            } else {
                format!(" {}", class.name)
            },
        });
    }
    let suspension = suspension::capture(module, &mut class, edits, &tokens);
    let private = PrivateNames::collect_with_origins(module, &class, emit.standard_output, false)?;
    let mut helpers = BTreeSet::new();
    if let Some(private) = &private {
        refuse_helper_name_collisions(module)?;
        private.rewrite_accesses(module, edits, &mut helpers)?;
    }
    lower_class(
        module,
        &class,
        emit,
        private.as_ref(),
        edits,
        auto_origins,
        replacement,
    )?;
    let mut before = String::new();
    let mut after = String::new();
    let mut inner = Vec::new();
    edits.retain(|edit| {
        if edit.start < class.span.start || edit.end > class.span.end {
            return true;
        }
        if edit.start == edit.end && edit.start == class.span.start {
            before.push_str(&edit.replacement);
        } else if edit.start == edit.end && edit.start == class.span.end {
            after.push_str(&edit.replacement);
        } else {
            inner.push(edit.clone());
        }
        false
    });
    let (body, nested) =
        render_relocated_span(&module.source, &inner, class.span.start, class.span.end);
    // The containing relocation may still reference nested, plain edits in
    // this original range. The outer edit owns their top-level application.
    edits.extend(nested);
    let variables = private
        .as_ref()
        .map(|private| format!("var {}; ", private.variables().join(", ")))
        .unwrap_or_default();
    let name = if expression.name.is_none() {
        let mut context = names::contextual_name(module, expression, &tokens);
        if context == "\"\"" {
            // Pinned TypeScript exposes its temporary constructor name when a
            // return-position anonymous class needs static-state lowering.
            let generated = if emit.es5 {
                let mut index = 1;
                loop {
                    let candidate = format!("class_{index}");
                    if !tokens.iter().any(|token| token.text == candidate) {
                        break candidate;
                    }
                    index += 1;
                }
            } else {
                names::anonymous_name(&tokens, emit.file_private_captures)
            };
            context = serde_json::to_string(&generated).unwrap();
        }
        format!(
            "Object.defineProperty({}, \"name\", {{value: {}, configurable: true}});",
            class.name, context
        )
    } else {
        String::new()
    };
    let mut declaration = if expression.statement {
        format!(
            "let {} = ",
            expression
                .name
                .as_ref()
                .expect("a class declaration is named")
        )
    } else {
        String::new()
    };
    let prefix_origin = names::evaluation_prefix_origin(expression, &tokens)
        .map(crate::emitter::relocations::source_anchor)
        .unwrap_or_default();
    let generated = if expression.statement {
        ""
    } else {
        crate::emitter::relocations::unmapped()
    };
    let mut completion_origin = String::new();
    if !expression.statement {
        if let Some(terminator) = tokens
            .get(tokens.partition_point(|token| token.start < class.span.end))
            .filter(|token| token.is(";"))
        {
            if emit.standard_output && name.is_empty() && after.starts_with(' ') {
                let method_value =
                    emit.es5 && class.members.iter().all(|member| member.field.is_none());
                let end = if method_value {
                    if module.source[class.span.end..terminator.start]
                        .chars()
                        .all(char::is_whitespace)
                    {
                        edits.push(TextEdit {
                            start: class.span.end,
                            end: terminator.start,
                            replacement: format!(
                                "{generated}{}{generated}",
                                &module.source[class.span.end..terminator.start]
                            ),
                        });
                    }
                    class.span.end
                } else {
                    terminator.end
                };
                completion_origin = crate::emitter::relocations::source_anchor(end);
            }
            edits.push(TextEdit {
                start: terminator.start,
                end: terminator.end,
                replacement: format!(
                    "{generated};{}",
                    crate::emitter::relocations::source_anchor(terminator.end)
                ),
            });
        }
    }
    let completion_gap = if emit.standard_output && name.is_empty() && after.starts_with(' ') {
        after.remove(0);
        " "
    } else {
        ""
    };
    let capture_completion = if name.is_empty() { "" } else { " " };
    let static_declaration = expression.statement
        && class.decorators.is_empty()
        && class.members.iter().all(|member| {
            member.decorators.is_empty()
                && !member
                    .name
                    .as_ref()
                    .is_some_and(|name| name.starts_with('#'))
                && member.static_block.is_none()
                && !member.key.first().is_some_and(|token| token.is("["))
        })
        && suspension.parameters.is_empty()
        && tokens
            .get(tokens.partition_point(|token| token.start < class.span.end))
            .is_some_and(|next| {
                module.source[class.span.end..next.start]
                    .chars()
                    .all(char::is_whitespace)
            })
        && class.members.iter().all(|member| {
            member.field.as_ref().is_none_or(|field| {
                field.is_static && !field.accessor && !field.name.starts_with('#')
            })
        })
        && class.members.iter().any(|member| {
            member
                .field
                .as_ref()
                .is_some_and(|field| field.is_static && field.initializer.is_some())
        });
    let capture_terminator = if static_declaration && !emit.es5 {
        format!(
            "{} ;{}",
            crate::emitter::relocations::unmapped(),
            crate::emitter::relocations::unmapped()
        )
    } else if static_declaration && emit.es5 {
        format!(
            "{} ;{}",
            crate::emitter::relocations::source_anchor(
                class
                    .extends_span
                    .as_ref()
                    .map_or(class.span.start, |span| span.end)
            ),
            crate::emitter::relocations::source_anchor(class.span.end)
        )
    } else {
        ";".to_string()
    };
    let mut finish_end = class.span.end;
    let mut finish_gap = String::new();
    let mut captured_origin = String::new();
    let mut finished = format!("{generated}return {};", class.name);
    let mut terminator = if expression.statement {
        ";".to_string()
    } else {
        String::new()
    };
    if static_declaration {
        let last_field = class
            .members
            .iter()
            .filter_map(|member| member.field.as_ref())
            .rfind(|field| field.is_static && field.initializer.is_some())
            .expect("static state");
        if !emit.es5 {
            declaration = format!(
                "{}let {}{}{} = ",
                crate::emitter::relocations::source_anchor(class.span.start),
                crate::emitter::relocations::source_anchor(class.name_span.start),
                class.name,
                crate::emitter::relocations::source_anchor(
                    class
                        .extends_span
                        .as_ref()
                        .and_then(|span| {
                            tokens[..tokens.partition_point(|token| token.start < span.start)]
                                .iter()
                                .rev()
                                .find(|token| token.is("extends"))
                                .map(|token| token.start)
                        })
                        .unwrap_or(class.body_span.start)
                )
            );
            captured_origin = crate::emitter::relocations::source_anchor(class.span.start);
            finished = format!(
                "{}return {};",
                crate::emitter::relocations::source_anchor(last_field.span.end),
                class.name
            );
        } else {
            finished = format!(
                "{}return {}{}; {}",
                crate::emitter::relocations::source_anchor(class.body_span.end - 1),
                class.name,
                crate::emitter::relocations::source_anchor(class.span.end),
                crate::emitter::relocations::unmapped()
            );
        }
        let value_end = if emit.es5 {
            class
                .extends_span
                .as_ref()
                .map_or(class.span.start, |span| span.end)
        } else {
            last_field.span.end
        };
        let statement_end = if emit.es5 {
            class.span.end
        } else {
            last_field.span.end
        };
        terminator = format!(
            "{} ;{}",
            crate::emitter::relocations::source_anchor(value_end),
            crate::emitter::relocations::source_anchor(statement_end)
        );
        if let Some(next) = tokens.get(tokens.partition_point(|token| token.start < class.span.end))
        {
            let trivia = &module.source[class.span.end..next.start];
            if trivia.chars().all(char::is_whitespace) {
                finish_end = next.start;
                finish_gap = format!(
                    "{}{}",
                    if trivia.is_empty() { " " } else { trivia },
                    crate::emitter::relocations::source_anchor(next.start)
                );
            }
        }
    }
    let effects_completion = if (static_declaration || !name.is_empty()) && !after.is_empty() {
        " "
    } else {
        ""
    };
    edits.push(TextEdit {
        start: class.span.start,
        end: finish_end,
        replacement: format!(
            "{prefix_origin}{declaration}(({}) => {{{variables}{}{before}{captured_origin}const {} = {body}{capture_terminator}{completion_origin}{completion_gap}{capture_completion}{generated}{name}{after}{effects_completion}{finished}}})({}){terminator}{generated}{finish_gap}",
            suspension.parameters.join(", "),
            helper_definitions(&helpers),
            class.name,
            suspension.arguments.join(", "),
        ),
    });
    Ok(())
}
