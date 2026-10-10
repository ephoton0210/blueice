// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Named ES5 classes after the existing AST-owned member and field lowering.

use super::*;

mod super_members;

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    let names = runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| {
            if let Declaration::Class(class) = declaration {
                Some(class.name.clone())
            } else {
                None
            }
        })
        .chain(
            module
                .class_expressions
                .values()
                .filter_map(|expression| expression.name.clone()),
        )
        .collect::<std::collections::BTreeSet<_>>();
    let mut remaining = options.limits.parser.max_tokens;
    let mut sequence = 0;
    loop {
        let tokens = crate::syntax::lex_with_limits(
            &module.id,
            &emitted.javascript,
            options.limits.parser.max_source_bytes,
            remaining,
        )
        .map_err(|mut diagnostics| diagnostics.remove(0))?;
        remaining = remaining.saturating_sub(tokens.len());
        let Some(start) = (0..tokens.len()).rev().find(|index| {
            tokens[*index].is("class")
                && tokens
                    .get(*index + 1)
                    .is_some_and(|token| names.contains(&token.text))
        }) else {
            return Ok(emitted);
        };
        let name = &tokens[start + 1].text;
        let extends = tokens
            .get(start + 2)
            .is_some_and(|token| token.is("extends"));
        let open = start + if extends { 4 } else { 2 };
        if !tokens.get(open).is_some_and(|token| token.is("{")) {
            return Err(unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "class heritage requires one retained base binding",
            ));
        }
        let close = exponentiation::matching_close(&tokens, open).ok_or_else(|| {
            unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "class body was not retained",
            )
        })?;
        let base = unused_name(&emitted.javascript, "base_class", &mut sequence);
        let inherit = if options.no_emit_helpers && !options.import_helpers {
            "__extends".to_string()
        } else {
            unused_name(&emitted.javascript, "inherit_class", &mut sequence)
        };
        let construct = if options.import_helpers || options.no_emit_helpers {
            "(function(base, receiver, values) { var result = Function.prototype.apply.call(base, receiver, values); return result !== null && (typeof result === 'object' || typeof result === 'function') ? result : receiver; })".to_string()
        } else {
            unused_name(&emitted.javascript, "call_base", &mut sequence)
        };
        let receiver = unused_name(&emitted.javascript, "class_this", &mut sequence);
        let mut constructor = None;
        let mut members = String::new();
        let mut cursor = open + 1;
        while cursor < close {
            if tokens[cursor].is(";") {
                cursor += 1;
                continue;
            }
            let is_static = tokens[cursor].is("static")
                && !tokens.get(cursor + 1).is_some_and(|token| token.is("("));
            let first = cursor + usize::from(is_static);
            let accessor = matches!(tokens[first].text.as_str(), "get" | "set")
                && !tokens.get(first + 1).is_some_and(|token| token.is("("));
            let property = first + usize::from(accessor);
            let property_end = if tokens[property].is("[") {
                exponentiation::matching_close(&tokens, property).ok_or_else(|| {
                    unsupported(
                        module,
                        &SourceSpan::new(&module.id, 0, 0),
                        "computed member key was not retained",
                    )
                })? + 1
            } else {
                property + 1
            };
            let parameters = property_end;
            if !tokens.get(parameters).is_some_and(|token| token.is("(")) {
                return Err(unsupported(
                    module,
                    &SourceSpan::new(&module.id, 0, 0),
                    "class member requires a retained named method",
                ));
            }
            let parameters_end = exponentiation::matching_close(&tokens, parameters).unwrap();
            let body = parameters_end + 1;
            if !tokens.get(body).is_some_and(|token| token.is("{")) {
                return Err(unsupported(
                    module,
                    &SourceSpan::new(&module.id, 0, 0),
                    "class method body was not retained",
                ));
            }
            let end = exponentiation::matching_close(&tokens, body).unwrap();
            let arguments =
                &emitted.javascript[tokens[parameters].end..tokens[parameters_end].start];
            let text = &emitted.javascript[tokens[body].start..tokens[end].end];
            if tokens[property].is("constructor") && !is_static && !accessor {
                let text = if extends {
                    derived_body(
                        &emitted.javascript,
                        &tokens,
                        (body, end),
                        &base,
                        &construct,
                        &receiver,
                        module,
                    )?
                } else {
                    text.to_string()
                };
                constructor = Some(format!("function {name}({arguments}) {text}\n"));
            } else {
                let owner = if is_static {
                    name.clone()
                } else {
                    format!("{name}.prototype")
                };
                let text = if extends {
                    super_members::body(
                        &emitted.javascript,
                        &tokens,
                        body,
                        end,
                        &base,
                        is_static,
                        module,
                    )?
                } else {
                    text.to_string()
                };
                let key = if tokens[property].is("[") {
                    format!(
                        "({})",
                        &emitted.javascript[tokens[property].end..tokens[property_end - 1].start]
                    )
                } else if tokens[property].kind == TokenKind::String {
                    tokens[property].text.clone()
                } else {
                    serde_json::to_string(&tokens[property].text).unwrap()
                };
                if accessor {
                    // Redefining only get or set retains the previously defined mate.
                    members.push_str(&format!("Object.defineProperty({owner}, {key}, {{ {}: function ({arguments}) {text}, enumerable: false, configurable: true }});\n",tokens[first].text));
                } else {
                    members.push_str(&format!("Object.defineProperty({owner}, {key}, {{ value: function ({arguments}) {text}, writable: true, enumerable: false, configurable: true }});\n"));
                }
            }
            cursor = end + 1;
        }
        let constructor = constructor.unwrap_or_else(|| {
            if extends {
                format!("function {name}() {{ return {construct}({base}, this, arguments); }}\n")
            } else {
                format!("function {name}() {{}}\n")
            }
        });
        let inheritance = if extends {
            format!("{inherit}({name}, {base});\n")
        } else {
            String::new()
        };
        let argument = if extends {
            tokens[start + 3].text.as_str()
        } else {
            ""
        };
        let expression = format!(
            "(function ({}) {{\n{constructor}{inheritance}{members}return {name};\n}})({argument})",
            if extends { base.as_str() } else { "" }
        );
        let replacement = if class_is_expression(&tokens, start) {
            format!("({expression})")
        } else {
            format!("var {name} = {expression};")
        };
        let insertion = directive_end(&tokens);
        let helper = if options.import_helpers {
            if !extends {
                String::new()
            } else if options.module_kind == crate::ModuleKind::CommonJs {
                format!("var {inherit} = require('tslib').__extends;")
            } else {
                format!("import {{__extends as {inherit}}} from 'tslib';")
            }
        } else if options.no_emit_helpers {
            String::new()
        } else {
            include_str!("../class_helpers.v1.js")
                .replace("__blueice_target_inherit_class", &inherit)
                .replace("__blueice_target_call_base", &construct)
        };
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!("\n{helper}\n"),
                },
                TextEdit {
                    start: tokens[start].start,
                    end: tokens[close].end,
                    replacement,
                },
            ],
        );
    }
}

/// A class expression produces a constructor value at its existing expression
/// site. Decorator lowering can introduce such a site for an original class
/// declaration, so classify the retained emitted syntax rather than its source
/// declaration alone.
fn class_is_expression(tokens: &[crate::Token], start: usize) -> bool {
    start.checked_sub(1).is_some_and(|before| {
        matches!(
            tokens[before].text.as_str(),
            "=" | "("
                | "["
                | ","
                | ":"
                | "?"
                | "return"
                | "new"
                | "=>"
                | "&&"
                | "||"
                | "??"
                | "+"
                | "-"
                | "!"
                | "~"
                | "void"
                | "typeof"
                | "yield"
                | "await"
        )
    })
}

fn derived_body(
    source: &str,
    tokens: &[Token],
    body: (usize, usize),
    base: &str,
    construct: &str,
    receiver: &str,
    module: &Module,
) -> Result<String, Diagnostic> {
    let (open, close) = body;
    let mut edits = Vec::new();
    let begin = tokens[open].start;
    let mut index = open + 1;
    while index < close {
        if tokens[index].is("function") {
            if let Some(body) = (index + 1..close).find(|index| tokens[*index].is("{")) {
                if let Some(end) = exponentiation::matching_close(tokens, body) {
                    index = end + 1;
                    continue;
                }
            }
        }
        if tokens[index].is("super") {
            if !tokens.get(index + 1).is_some_and(|token| token.is("(")) {
                return Err(unsupported(
                    module,
                    &SourceSpan::new(&module.id, 0, 0),
                    "derived constructor super properties require a retained receiver",
                ));
            }
            let end = exponentiation::matching_close(tokens, index + 1).unwrap();
            let arguments = &tokens[index + 2..end];
            let values = if let [spread, arguments] = arguments {
                if spread.is("...") && arguments.is("arguments") {
                    "arguments".to_string()
                } else {
                    format!("[{}]", &source[tokens[index + 1].end..tokens[end].start])
                }
            } else {
                format!("[{}]", &source[tokens[index + 1].end..tokens[end].start])
            };
            edits.push(TextEdit {
                start: tokens[index].start - begin,
                end: tokens[end].end - begin,
                replacement: format!("{receiver} = {construct}({base}, this, {values})"),
            });
            index = end + 1;
            continue;
        }
        if tokens[index].is("this") {
            edits.push(TextEdit {
                start: tokens[index].start - begin,
                end: tokens[index].end - begin,
                replacement: receiver.into(),
            });
        }
        if tokens[index].is("return") {
            return Err(unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "derived constructor returns require a retained constructor completion",
            ));
        }
        index += 1;
    }
    edits.push(TextEdit {
        start: tokens[open].end - begin,
        end: tokens[open].end - begin,
        replacement: format!("\nvar {receiver};\n"),
    });
    edits.push(TextEdit {
        start: tokens[close].start - begin,
        end: tokens[close].start - begin,
        replacement: format!("\nreturn {receiver};\n"),
    });
    Ok(apply_edits(&source[begin..tokens[close].end], edits).javascript)
}
