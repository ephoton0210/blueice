// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Flat named object-rest bindings below ES2018. The retained ordinary
//! pattern performs defaults before the rest copy, without repeating reads.

use super::*;

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
    if options.target >= crate::EcmaTarget::Es2018 {
        return Ok(emitted);
    }
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
        let Some(pattern) = find_pattern(&tokens) else {
            return Ok(emitted);
        };
        let start = pattern.close + 2;
        let end = assignment_end(&tokens, start);
        if end <= start {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                SourceSpan::new(&module.id, 0, 0),
                "target object-rest lowering requires an initializer",
            ));
        }
        let temporary = unused_name(&emitted.javascript, "rest_source", &mut sequence);
        let external = super::super::helper_selection::external(options);
        let helper = if external {
            super::super::helper_selection::reference(&emitted.javascript, "__rest")
        } else {
            unused_name(&emitted.javascript, "rest_properties", &mut sequence)
        };
        let initializer_start = tokens[start].start;
        let initializer_end = tokens[end - 1].end;
        let initializer =
            super::super::relocations::relocated_verbatim(initializer_start, initializer_end);
        let stable = end == start + 1 && !pattern.keys.is_empty() && module.declarations.iter().any(|declaration| matches!(declaration, Declaration::Variable(variable) if variable.kind == crate::parser::VariableKind::Const && variable.name == tokens[start].text));
        let inner_end = if tokens[pattern.rest - 1].is(",") {
            tokens[pattern.rest - 1].start
        } else {
            tokens[pattern.rest].start
        };
        let ordinary =
            super::super::relocations::relocated_verbatim(tokens[pattern.open].end, inner_end);
        let excluded = serde_json::to_string(&pattern.keys).expect("binding keys are serializable");
        let name = &tokens[pattern.rest + 1].text;
        let anchor = super::super::relocations::source_anchor;
        let generated = super::super::relocations::unmapped();
        let ordinary = format!(
            "{}{{{ordinary}{}}}{}",
            anchor(tokens[pattern.open].start),
            anchor(inner_end),
            anchor(inner_end)
        );
        let capture = if stable {
            String::new()
        } else {
            format!("{generated}{temporary} = {initializer}, ")
        };
        let operand = if stable {
            initializer
        } else {
            format!("{generated}{temporary}")
        };
        let ordinary_binding = format!("{ordinary} = {operand}{}, ", anchor(initializer_end));
        let rest_name = format!(
            "{}{name}{}",
            anchor(tokens[pattern.rest + 1].start),
            anchor(tokens[pattern.rest + 1].end)
        );
        let arguments = if external {
            format!(
                "{operand}{}, {}{excluded}",
                anchor(initializer_end),
                anchor(tokens[pattern.open].start)
            )
        } else {
            format!(
                "{generated}{{}}, {operand}{},{generated} true, {}{excluded}",
                anchor(initializer_end),
                anchor(tokens[pattern.open].start)
            )
        };
        let replacement = format!(
            "{capture}{ordinary_binding}{rest_name} = {helper}({arguments}{})",
            anchor(tokens[pattern.close].end)
        );
        let endpoint = if options.target == crate::EcmaTarget::Es5 {
            tokens[pattern.rest + 1].end
        } else {
            initializer_end
        };
        let insertion = directive_end(&tokens);
        emitted = super::mapped_fragments::with_completion(
            emitted,
            vec![
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: if external {
                        String::new()
                    } else {
                        format!(
                            "{generated}\n{}\n",
                            object_spread::HELPER
                                .replace("__blueice_target_object_properties", &helper)
                        )
                    },
                },
                TextEdit {
                    start: tokens[pattern.open].start,
                    end: tokens[end - 1].end,
                    replacement,
                },
            ],
            initializer_end,
            endpoint,
        );
    }
}

struct Pattern {
    open: usize,
    close: usize,
    rest: usize,
    keys: Vec<String>,
}

fn find_pattern(tokens: &[Token]) -> Option<Pattern> {
    for open in 1..tokens.len() {
        if !tokens[open].is("{")
            || !matches!(tokens[open - 1].text.as_str(), "const" | "let" | "var")
        {
            continue;
        }
        let mut depth = 0usize;
        let mut first = true;
        let mut keys = Vec::new();
        for index in open + 1..tokens.len() {
            let token = &tokens[index];
            if depth == 0 && first {
                if token.is("...") {
                    let name = tokens.get(index + 1)?;
                    if name.kind == TokenKind::Identifier
                        && tokens.get(index + 2)?.is("}")
                        && tokens.get(index + 3)?.is("=")
                    {
                        return Some(Pattern {
                            open,
                            close: index + 2,
                            rest: index,
                            keys,
                        });
                    }
                    break;
                }
                if !matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword) {
                    break;
                }
                keys.push(token.text.clone());
                first = false;
            }
            match token.text.as_str() {
                "}" if depth == 0 => break,
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.checked_sub(1)?,
                "," if depth == 0 => first = true,
                _ => {}
            }
        }
    }
    None
}
