// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Direct templates retain original byte offsets in embedded expressions.

use super::*;

impl ExpressionLowerer<'_> {
    pub(super) fn lower_template(&mut self, token: &Token) -> Result<bluejs::Expr, BridgeError> {
        let module = self.module;
        let body = token
            .text
            .strip_prefix('`')
            .and_then(|text| text.strip_suffix('`'))
            .ok_or_else(|| unsupported(token_span(module, token), "invalid template token"))?;
        let mut quasis = Vec::new();
        let mut expressions = Vec::new();
        let mut remainder = body;
        let mut remainder_offset = 1usize;
        while let Some(start) = template_substitution_start(remainder) {
            quasis.push(decode_string_escapes(module, token, &remainder[..start])?.into());
            let expression_start = start + 2;
            let Some(end) = template_substitution_end(&remainder[expression_start..]) else {
                return Err(unsupported(
                    token_span(module, token),
                    "unterminated template substitution",
                ));
            };
            let expression = &remainder[expression_start..expression_start + end];
            expressions.push(self.lower_template_substitution(
                token,
                expression,
                remainder_offset + expression_start,
            )?);
            remainder_offset += expression_start + end + 1;
            remainder = &remainder[expression_start + end + 1..];
        }
        quasis.push(decode_string_escapes(module, token, remainder)?.into());
        Ok(bluejs::Expr::Template {
            quasis,
            expressions,
        })
    }

    fn lower_template_substitution(
        &mut self,
        template: &Token,
        source: &str,
        source_offset: usize,
    ) -> Result<bluejs::Expr, BridgeError> {
        let module = self.module;
        if source.is_empty() {
            return Err(unsupported(
                token_span(module, template),
                "empty template substitutions are not expressions",
            ));
        }
        let mut tokens = lex(module, source).map_err(|_| {
            unsupported(
                token_span(module, template),
                "invalid expression in a template substitution",
            )
        })?;
        let eof = tokens
            .pop()
            .expect("BlueTS lexer always terminates with EOF");
        debug_assert_eq!(eof.kind, TokenKind::Eof);
        if tokens.is_empty() {
            return Err(unsupported(
                token_span(module, template),
                "empty template substitutions are not expressions",
            ));
        }
        for token in &mut tokens {
            token.start += template.start + source_offset;
            token.end += template.start + source_offset;
        }
        if let Some(optional) = tokens.iter().find(|token| token.is("?.")) {
            return Err(unsupported(
                token_span(module, optional),
                "unsupported expression token `?.` in a template substitution",
            ));
        }
        self.parse_embedded_expression(&tokens)
    }
}

pub(super) fn template_substitution_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
        } else if bytes[index] == b'$' && bytes[index + 1] == b'{' {
            return Some(index);
        } else {
            index += 1;
        }
    }
    None
}

pub(super) fn template_substitution_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut depth = 1usize;
    let mut quote = None;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index] == delimiter {
                quote = None;
            }
            index += 1;
            continue;
        }
        match bytes[index] {
            b'\'' | b'"' => quote = Some(bytes[index]),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}
