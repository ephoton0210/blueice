// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded constant-template types, using only already checked literal values.

use super::*;
use crate::enum_eval::{
    decode_plain_string, evaluate_literal_expression, js_number_text, parse_number, EnumValue,
};

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn constant_template_type(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let [token] = strip_outer_parentheses(tokens) else {
            return None;
        };
        if token.kind != TokenKind::Template {
            return None;
        }
        let text = token.text.strip_prefix('`')?.strip_suffix('`')?;
        let bytes = text.as_bytes();
        let mut result = String::new();
        let mut start = 0;
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'\\' {
                index = (index + 2).min(bytes.len());
                continue;
            }
            if bytes[index..].starts_with(b"${") {
                result.push_str(&cooked(&text[start..index])?);
                let expression = index + 2;
                let (end, tokens) =
                    crate::syntax::lex_expression(&self.module.id, text, expression).ok()?;
                if tokens.len() > self.max_type_expansions.min(128) {
                    return None;
                }
                let literals: BTreeMap<_, _> = tokens
                    .iter()
                    .filter_map(|token| {
                        let Type::Literal(value) = scope.get(&token.text)? else {
                            return None;
                        };
                        let value = if value.starts_with(['\'', '"']) {
                            EnumValue::Text(decode_plain_string(value)?)
                        } else {
                            EnumValue::Number(parse_number(value)?)
                        };
                        Some((token.text.clone(), value))
                    })
                    .collect();
                match evaluate_literal_expression(&tokens, &literals)? {
                    EnumValue::Number(value) => result.push_str(&js_number_text(value)),
                    EnumValue::Text(value) => result.push_str(&value),
                }
                index = end + 1;
                start = index;
            } else {
                index += 1;
            }
        }
        result.push_str(&cooked(&text[start..])?);
        Some(Type::Literal(serde_json::to_string(&result).ok()?))
    }
}

fn cooked(text: &str) -> Option<String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    decode_plain_string(&format!("\"{normalized}\""))
}
