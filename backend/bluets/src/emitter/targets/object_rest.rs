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
        let helper = unused_name(&emitted.javascript, "rest_properties", &mut sequence);
        let initializer = &emitted.javascript[tokens[start].start..tokens[end - 1].end];
        let inner_end = if tokens[pattern.rest - 1].is(",") {
            tokens[pattern.rest - 1].start
        } else {
            tokens[pattern.rest].start
        };
        let ordinary = &emitted.javascript[tokens[pattern.open].end..inner_end];
        let excluded = serde_json::to_string(&pattern.keys).expect("binding keys are serializable");
        let name = &tokens[pattern.rest + 1].text;
        let replacement = format!(
            "{temporary} = {initializer}, {{{ordinary}}} = {temporary}, \
             {name} = {helper}({{}}, {temporary}, true, {excluded})"
        );
        let insertion = directive_end(&tokens);
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!(
                        "\n{}\n",
                        object_spread::HELPER
                            .replace("__blueice_target_object_properties", &helper)
                    ),
                },
                TextEdit {
                    start: tokens[pattern.open].start,
                    end: tokens[end - 1].end,
                    replacement,
                },
            ],
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
