// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration initializers retain their full span; assignment calls end at
//! their final substitution. System moves top-level declarations to assignments.
use super::*;

pub(super) fn completion(
    tokens: &[Token],
    template: &Token,
    expression_end: usize,
    options: &CompilerOptions,
) -> usize {
    let index = tokens.partition_point(|token| token.start < template.start);
    if !tokens
        .get(index + 1)
        .is_some_and(|token| token.is(";") || token.is(",") || token.kind == TokenKind::Eof)
    {
        return expression_end;
    }
    let mut depth = 0usize;
    for cursor in (0..index).rev() {
        match tokens[cursor].text.as_str() {
            ")" | "]" | "}" => depth += 1,
            "(" | "[" | "{" if depth > 0 => depth -= 1,
            "(" | "[" | "{" | ";" | "return" | "throw" if depth == 0 => break,
            "var" | "let" | "const" if depth == 0 => {
                let mut nesting = 0usize;
                for token in &tokens[..cursor] {
                    match token.text.as_str() {
                        "(" | "[" | "{" => nesting += 1,
                        ")" | "]" | "}" => nesting = nesting.saturating_sub(1),
                        _ => {}
                    }
                }
                return if options.module_kind == crate::ModuleKind::System && nesting == 0 {
                    expression_end
                } else {
                    template.end
                };
            }
            _ => {}
        }
    }
    expression_end
}
