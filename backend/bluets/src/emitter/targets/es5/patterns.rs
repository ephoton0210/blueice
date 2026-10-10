// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reuse flat binding records after erasure and object-rest lowering.

use super::*;
use crate::parser::{parse_variable_pattern, BindingPattern};

pub(super) fn lower(
    mut emitted: EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<EmittedJavaScript, Diagnostic> {
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
        let pattern = tokens.iter().enumerate().find_map(|(index, token)| {
            if !matches!(tokens.get(index + 1)?.text.as_str(), "{" | "[") {
                return None;
            }
            if !(matches!(token.text.as_str(), "var" | "let" | "const")
                || token.is(",") && in_declaration(&tokens, index))
            {
                return None;
            }
            let (pattern, close) = parse_variable_pattern(&tokens, index + 1, &module.id)?;
            if !tokens.get(close + 1)?.is("=")
                || (options.downlevel_iteration
                    && matches!(pattern.pattern, BindingPattern::Array(_)))
            {
                return None;
            }
            Some((index + 1, close, pattern))
        });
        let Some((open, close, pattern)) = pattern else {
            return Ok(emitted);
        };
        let end = assignment_end(&tokens, close + 2);
        if end <= close + 2 {
            return Err(unsupported(
                module,
                &SourceSpan::new(&module.id, 0, 0),
                "binding pattern initializer was not retained",
            ));
        }
        let source = &emitted.javascript[tokens[close + 2].start..tokens[end - 1].end];
        let temporary = unused_name(&emitted.javascript, "pattern_source", &mut sequence);
        let mut replacement = format!("{temporary} = {source}");
        let mut helper = None;
        match &pattern.pattern {
            BindingPattern::Object(bindings) => {
                let require = unused_name(&emitted.javascript, "require_object", &mut sequence);
                replacement = format!("{temporary} = {require}({source})");
                helper = Some(
                    include_str!("../es5_helpers.v1.js")
                        .replace("__blueice_target_require_object", &require),
                );
                for binding in bindings {
                    add_binding(
                        &mut replacement,
                        &binding.name,
                        &format!("{temporary}.{}", binding.key),
                        binding.default.as_deref(),
                        &emitted.javascript,
                        &mut sequence,
                    );
                }
                if pattern.rest.is_some() {
                    return Err(unsupported(
                        module,
                        &SourceSpan::new(&module.id, 0, 0),
                        "object rest was not lowered before ES5 binding projection",
                    ));
                }
            }
            BindingPattern::Array(bindings) => {
                for (index, binding) in bindings.iter().enumerate() {
                    if let Some(binding) = binding {
                        add_binding(
                            &mut replacement,
                            &binding.name,
                            &format!("{temporary}[{index}]"),
                            binding.default.as_deref(),
                            &emitted.javascript,
                            &mut sequence,
                        );
                    }
                }
                if let Some(rest) = &pattern.rest {
                    replacement.push_str(&format!(
                        ", {} = {temporary}.slice({})",
                        rest.name,
                        bindings.len()
                    ));
                }
            }
        }
        let mut edits = vec![TextEdit {
            start: tokens[open].start,
            end: tokens[end - 1].end,
            replacement,
        }];
        if let Some(helper) = helper {
            let insertion = directive_end(&tokens);
            edits.push(TextEdit {
                start: insertion,
                end: insertion,
                replacement: format!("\n{helper}\n"),
            });
        }
        emitted = mapped_edits(emitted, edits);
    }
}

fn in_declaration(tokens: &[Token], before: usize) -> bool {
    let mut depth = 0usize;
    for token in tokens[..before].iter().rev() {
        match token.text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth > 0 => depth -= 1,
            "(" | "[" | "{" | ";" if depth == 0 => return false,
            "var" | "let" | "const" if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn add_binding(
    output: &mut String,
    name: &str,
    read: &str,
    default: Option<&[Token]>,
    source: &str,
    sequence: &mut usize,
) {
    if let Some((first, last)) = default.and_then(|tokens| Some((tokens.first()?, tokens.last()?)))
    {
        let value = unused_name(source, "pattern_value", sequence);
        output.push_str(&format!(
            ", {value} = {read}, {name} = {value} === void 0 ? ({}) : {value}",
            &source[first.start..last.end]
        ));
    } else {
        output.push_str(&format!(", {name} = {read}"));
    }
}
