// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Literal-valued object fields have a static type before class binding.
use super::*;

pub(super) fn record(tokens: &[Token], module: &str) -> Option<Type> {
    if !tokens.first()?.is("{") || !tokens.last()?.is("}") {
        return None;
    }
    let mut index = 1;
    let mut fields: Vec<TypeField> = Vec::new();
    while index < tokens.len() - 1 {
        let key = tokens.get(index)?;
        if !matches!(
            key.kind,
            TokenKind::Identifier | TokenKind::Keyword | TokenKind::String | TokenKind::Number
        ) {
            return None;
        }
        if !tokens.get(index + 1)?.is(":") {
            return None;
        }
        let start = index + 2;
        index = start;
        while index < tokens.len() - 1 && !tokens[index].is(",") {
            index += 1
        }
        let value = widen_literal_tokens(&tokens[start..index], false)?;
        let name = key.text.trim_matches(['\'', '"']).to_string();
        if fields.iter().any(|field| field.name == name) {
            return None;
        }
        fields.push(TypeField {
            accessor_write_type: None,
            method: false,
            name,
            readonly: false,
            optional: false,
            value,
            span: SourceSpan::new(module, key.start, tokens[index - 1].end),
        });
        if tokens.get(index).is_some_and(|token| token.is(",")) {
            index += 1
        }
    }
    Some(Type::Record(fields))
}
