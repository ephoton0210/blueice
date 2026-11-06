// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Local export keys retain their original identifier or string-literal syntax.
use crate::parser::{Module, ValueExportBinding};
use crate::TokenKind;

pub(super) struct Key {
    pub(super) member: String,
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) fn key(module: &Module, binding: &ValueExportBinding) -> Key {
    let tokens = crate::lex(
        &module.id,
        &module.source[binding.span.start..binding.span.end],
    )
    .unwrap_or_default();
    let name = tokens
        .iter()
        .rev()
        .find(|token| token.kind != TokenKind::Eof);
    let identifier = name.is_some_and(|token| {
        matches!(token.kind, TokenKind::Identifier | TokenKind::Keyword)
            && token.text == binding.exported
    });
    Key {
        member: if identifier {
            format!("exports.{}", binding.exported)
        } else {
            format!("exports[{:?}]", binding.exported)
        },
        start: name.map_or(binding.span.start, |token| binding.span.start + token.start),
        end: binding.span.end,
    }
}

#[cfg(test)]
mod tests {
    use super::key;
    use crate::{parse_module, Declaration};

    #[test]
    fn export_property_syntax_follows_original_tokens_instead_of_decoded_name_guessing() {
        for (spelling, expected) in [
            ("Alias", "exports.Alias"),
            ("default", "exports.default"),
            ("\"Alias\"", "exports[\"Alias\"]"),
            ("\"x-y\"", "exports[\"x-y\"]"),
        ] {
            let source = format!("export {{ Model as {spelling} }};");
            let module = parse_module("memory:///main.ts", &source).unwrap();
            let Declaration::ValueExport(export) = &module.declarations[0] else {
                panic!("original export declaration");
            };
            let property = key(&module, &export.bindings[0]);
            assert_eq!(property.member, expected);
            assert_eq!(&source[property.start..property.end], spelling);
        }
    }
}
