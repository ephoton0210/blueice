// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A retained runtime class reference keeps its annotation's original tokens.
use super::{Module, Parameter, SourceSpan};
use crate::emitter::relocations::source_anchor;

pub(super) fn static_completion(
    class: &crate::parser::ClassDeclaration,
    options: &crate::CompilerOptions,
) -> String {
    if options.target >= crate::EcmaTarget::Es2022 {
        return String::new();
    }
    for member in class.members.iter().rev() {
        if member.static_block.is_some() {
            return String::new();
        }
        if let Some(field) = member.field.as_ref().filter(|field| {
            field.is_static
                && !field.declared
                && (field.initializer.is_some() || options.defines_class_fields())
        }) {
            return if field.accessor || field.name.starts_with('#') {
                String::new()
            } else {
                source_anchor(field.span.end)
            };
        }
    }
    String::new()
}

pub(super) fn parameter_span(module: &Module, parameter: &Parameter) -> SourceSpan {
    let source = &module.source[parameter.span.start..parameter.span.end];
    let tokens = crate::lex(&module.id, source).unwrap_or_default();
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        if token.is(":") && depth == 0 {
            if let Some(next) = tokens.get(index + 1) {
                return SourceSpan::new(
                    &module.id,
                    parameter.span.start + next.start,
                    parameter.span.end,
                );
            }
        }
        if matches!(token.text.as_str(), "(" | "[" | "{") {
            depth += 1;
        } else if matches!(token.text.as_str(), ")" | "]" | "}") {
            depth = depth.saturating_sub(1);
        }
    }
    parameter.span.clone()
}

pub(super) fn named(module: &Module, name: &str, serialized: &str, span: &SourceSpan) -> String {
    let source = &module.source[span.start..span.end];
    let tokens = crate::lex(&module.id, source).unwrap_or_default();
    let wanted = name.split('.').collect::<Vec<_>>();
    let count = wanted.len() * 2 - 1;
    if tokens.len() < count
        || !tokens[..count].iter().enumerate().all(|(index, token)| {
            if index % 2 == 0 {
                token.text == wanted[index / 2]
            } else {
                token.is(".")
            }
        })
    {
        return serialized.to_string();
    }
    if serialized != name {
        // A named import is replaced by a fresh module namespace access. The
        // entire generated prefix inherits the original imported identifier.
        return format!(
            "{}{serialized}{}",
            source_anchor(span.start + tokens[0].start),
            source_anchor(span.start + tokens[count - 1].end),
        );
    }
    let mut result = String::new();
    for token in &tokens[..count] {
        result.push_str(&source_anchor(span.start + token.start));
        result.push_str(&token.text);
        result.push_str(&source_anchor(span.start + token.end));
    }
    result
}
