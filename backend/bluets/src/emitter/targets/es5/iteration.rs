// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Braced for-of loops keep abrupt completion and iterator closing in place.

use super::*;
use crate::emitter::relocations;

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
        let Some(loop_) = (0..tokens.len())
            .rev()
            .find_map(|index| braced_loop(&tokens, index))
        else {
            return Ok(emitted);
        };
        let source = relocations::relocated_verbatim(
            tokens[loop_.of + 1].start,
            tokens[loop_.close - 1].end,
        );
        let binding =
            relocations::relocated_verbatim(tokens[loop_.binding].start, tokens[loop_.of].start);
        let body = relocations::relocated_verbatim(tokens[loop_.body].start, tokens[loop_.end].end);
        let generated = relocations::unmapped();
        let entry = relocations::source_anchor(tokens[loop_.start].start);
        let operand = relocations::source_anchor(tokens[loop_.of + 1].start);
        let operand_end = relocations::source_anchor(tokens[loop_.close - 1].end);
        let binding_end = relocations::source_anchor(tokens[loop_.of - 1].end);
        let record = unused_name(&emitted.javascript, "iterator_record", &mut sequence);
        let result = unused_name(&emitted.javascript, "iterator_step", &mut sequence);
        let mut helper = String::new();
        let mut replacement = if options.downlevel_iteration {
            let finished = unused_name(&emitted.javascript, "iterator_finished", &mut sequence);
            let abrupt = unused_name(&emitted.javascript, "iterator_abrupt", &mut sequence);
            let error = unused_name(&emitted.javascript, "iterator_error", &mut sequence);
            let caught = unused_name(&emitted.javascript, "iterator_caught", &mut sequence);
            let closing = unused_name(&emitted.javascript, "iterator_closing", &mut sequence);
            let close_error =
                unused_name(&emitted.javascript, "iterator_close_error", &mut sequence);
            let value = unused_name(&emitted.javascript, "iterator_value", &mut sequence);
            let iterator_helper = unused_name(&emitted.javascript, "iterator", &mut sequence);
            let result_helper = unused_name(&emitted.javascript, "iterator_result", &mut sequence);
            let external = crate::emitter::helper_selection::external(options);
            let (factory, step_call, receiver) = if external {
                (
                    crate::emitter::helper_selection::reference(&emitted.javascript, "__values"),
                    format!("{operand_end}{record}.next()"),
                    record.clone(),
                )
            } else {
                helper = include_str!("../iteration_helpers.v1.js")
                    .replace("__blueice_target_iterator", &iterator_helper)
                    .replace("__blueice_target_step_result", &result_helper);
                (iterator_helper, format!("{generated}{result_helper}({operand_end}{record}.next.call({record}.iterator){generated}){operand_end}"), format!("{record}.iterator"))
            };
            let closing_call = if external {
                format!("{closing}.call({receiver})")
            } else {
                format!("{result_helper}({closing}.call({receiver}))")
            };
            format!("{generated}{{\n{operand}var {record} = {factory}({source}{operand_end});\n{generated}var {result}, {value}, {finished}=true, {abrupt}=false, {error};\ntry {{\n{entry}for ({operand_end}; !({result}={step_call}).done; {generated}{finished}=true{operand_end}){generated} {{\n{value}={binding_end}{result}.value{binding_end};{generated} {finished}=false; {binding}{binding_end}={value};\n{body}\n{generated}}}\n}} catch({caught}) {{ {abrupt}=true; {error}={caught}; }}\nfinally {{ if(!{finished}) {{ try {{ var {closing}={receiver}.return; if({closing}!==null&&{closing}!==void 0) {closing_call}; }} catch({close_error}) {{ if(!{abrupt}) throw {close_error}; }} }} if({abrupt}) throw {error}; }}\n}}")
        } else {
            format!("{{ var {record} = {source}; for (var {result} = 0; {result} < {record}.length; {result}++) {{ {binding} = {record}[{result}]; {body} }} }}")
        };
        let mut end = tokens[loop_.end].end;
        if let Some(next) = tokens.get(loop_.end + 1) {
            let trivia = &emitted.javascript[end..next.start];
            if trivia.chars().all(char::is_whitespace) {
                replacement.push_str(generated);
                replacement.push_str(trivia);
                end = next.start;
            }
        }
        let mut edits = vec![TextEdit {
            start: tokens[loop_.start].start,
            end,
            replacement,
        }];
        if !helper.is_empty() {
            let insertion = directive_end(&tokens);
            edits.push(TextEdit {
                start: insertion,
                end: insertion,
                replacement: format!("{generated}\n{helper}\n"),
            });
        }
        emitted = mapped_edits(emitted, edits);
    }
}

struct Loop {
    start: usize,
    binding: usize,
    of: usize,
    close: usize,
    body: usize,
    end: usize,
}

fn braced_loop(tokens: &[Token], start: usize) -> Option<Loop> {
    if !tokens[start].is("for")
        || !tokens.get(start + 1)?.is("(")
        || start > 0 && tokens[start - 1].is(":")
    {
        return None;
    }
    let binding = start + 2;
    let name = binding
        + usize::from(matches!(
            tokens.get(binding)?.text.as_str(),
            "var" | "let" | "const"
        ));
    if tokens.get(name)?.kind != TokenKind::Identifier || !tokens.get(name + 1)?.is("of") {
        return None;
    }
    let of = name + 1;
    let close = exponentiation::matching_close(tokens, start + 1)?;
    if close <= of + 1 || !tokens.get(close + 1)?.is("{") {
        return None;
    }
    let body = close + 1;
    Some(Loop {
        start,
        binding,
        of,
        close,
        body,
        end: exponentiation::matching_close(tokens, body)?,
    })
}
