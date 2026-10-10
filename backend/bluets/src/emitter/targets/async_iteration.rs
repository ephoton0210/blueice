// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Braced for-await loops retain their original completion in try/finally.
//! A body throw wins over a closing failure; break/return validate the result.

use super::*;

const HELPER: &str = include_str!("async_iteration_helpers.v1.js");

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
        let Some(loop_) = (0..tokens.len())
            .rev()
            .find_map(|index| braced_loop(&tokens, index))
        else {
            return Ok(emitted);
        };
        let record = unused_name(&emitted.javascript, "async_record", &mut sequence);
        let result = unused_name(&emitted.javascript, "async_result", &mut sequence);
        let value = unused_name(&emitted.javascript, "async_value", &mut sequence);
        let finished = unused_name(&emitted.javascript, "async_finished", &mut sequence);
        let abrupt = unused_name(&emitted.javascript, "async_abrupt", &mut sequence);
        let error = unused_name(&emitted.javascript, "async_error", &mut sequence);
        let caught = unused_name(&emitted.javascript, "async_caught", &mut sequence);
        let closing = unused_name(&emitted.javascript, "async_closing", &mut sequence);
        let close_error = unused_name(&emitted.javascript, "async_close_error", &mut sequence);
        let iterator_helper = unused_name(&emitted.javascript, "async_iterator", &mut sequence);
        let result_helper = unused_name(&emitted.javascript, "iterator_result", &mut sequence);
        let iterable = &emitted.javascript[tokens[loop_.iterable].start..tokens[loop_.close].start];
        let binding = &emitted.javascript[tokens[loop_.binding].start..tokens[loop_.of].start];
        let body = &emitted.javascript[tokens[loop_.body].start..tokens[loop_.end].end];
        let body = if options.target == crate::EcmaTarget::Es5 {
            // The outer braces carry no lexical declarations after ES5's
            // binding-identity pass; omit only this redundant nested block.
            &emitted.javascript[tokens[loop_.body].end..tokens[loop_.end].start]
        } else {
            body
        };
        let replacement = format!(
            "{{\nvar {record} = {iterator_helper}({iterable});\n\
             var {result}, {value}, {finished} = true, {abrupt} = false, {error};\n\
             try {{\n\
             for (; !({result} = {result_helper}(await {record}.next.call({record}.iterator))).done; \
                    {finished} = true) {{\n\
               {value} = {result}.value;\n\
               {finished} = false;\n\
               {binding} = {value};\n\
               {body}\n\
             }}\n\
             }} catch ({caught}) {{ {abrupt} = true; {error} = {caught}; }}\n\
             finally {{\n\
               if (!{finished}) {{\n\
                 try {{\n\
                   var {closing} = {record}.iterator.return;\n\
                   if ({closing} !== null && {closing} !== void 0) {{\n\
                     {result_helper}(await {closing}.call({record}.iterator));\n\
                   }}\n\
                 }} catch ({close_error}) {{ if (!{abrupt}) throw {close_error}; }}\n\
               }}\n\
               if ({abrupt}) throw {error};\n\
             }}\n}}"
        );
        let helper = HELPER
            .replace("__blueice_target_async_iterator", &iterator_helper)
            .replace("__blueice_target_iterator_result", &result_helper);
        let insertion = directive_end(&tokens);
        emitted = mapped_edits(
            emitted,
            vec![
                TextEdit {
                    start: insertion,
                    end: insertion,
                    replacement: format!("\n{helper}\n"),
                },
                TextEdit {
                    start: tokens[loop_.start].start,
                    end: tokens[loop_.end].end,
                    replacement,
                },
            ],
        );
    }
}

struct Loop {
    start: usize,
    binding: usize,
    of: usize,
    iterable: usize,
    close: usize,
    body: usize,
    end: usize,
}

fn braced_loop(tokens: &[Token], start: usize) -> Option<Loop> {
    if !tokens[start].is("for")
        || !tokens.get(start + 1)?.is("await")
        || !tokens.get(start + 2)?.is("(")
        || start > 0 && tokens[start - 1].is(":")
    {
        return None;
    }
    let binding = start + 3;
    let name = binding
        + usize::from(matches!(
            tokens.get(binding)?.text.as_str(),
            "const" | "let" | "var"
        ));
    if tokens.get(name)?.kind != TokenKind::Identifier || !tokens.get(name + 1)?.is("of") {
        return None;
    }
    let of = name + 1;
    let iterable = of + 1;
    let close = exponentiation::matching_close(tokens, start + 2)?;
    if close <= iterable || !tokens.get(close + 1)?.is("{") {
        return None;
    }
    let body = close + 1;
    Some(Loop {
        start,
        binding,
        of,
        iterable,
        close,
        body,
        end: exponentiation::matching_close(tokens, body)?,
    })
}
