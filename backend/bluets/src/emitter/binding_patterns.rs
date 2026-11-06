// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration patterns keep binding structure while dropping runtime defaults.
use crate::parser::BindingPattern;

pub(super) fn declaration(pattern: &BindingPattern) -> String {
    match pattern {
        BindingPattern::Object(bindings) => format!(
            "{{ {} }}",
            bindings
                .iter()
                .map(|binding| {
                    if let Some(nested) = &binding.nested {
                        format!("{}: {}", binding.key, declaration(&nested.pattern))
                    } else if binding.key == binding.name {
                        binding.name.clone()
                    } else {
                        format!("{}: {}", binding.key, binding.name)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        BindingPattern::Array(bindings) => format!(
            "[{}]",
            bindings
                .iter()
                .map(|binding| binding
                    .as_ref()
                    .map(|binding| binding.nested.as_ref().map_or_else(
                        || binding.name.clone(),
                        |nested| declaration(&nested.pattern)
                    ))
                    .unwrap_or_default())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Binding openers begin a source node even when declaration trivia precedes it.
pub(super) fn javascript_anchors(
    module: &crate::parser::Module,
    tokens: &[crate::Token],
    anchors: &mut std::collections::BTreeMap<usize, Option<usize>>,
) {
    for (name, operator) in for_of_operators(tokens) {
        anchors.insert(operator.start, Some(name.end));
    }
    for token in tokens.iter().filter(|token| token.is("...")) {
        anchors.insert(token.start, Some(token.start));
    }
    for (index, token) in tokens.iter().enumerate().skip(1) {
        if matches!(token.text.as_str(), "{" | "[")
            && matches!(tokens[index - 1].text.as_str(), "const" | "let" | "var")
            && (index < 2 || !matches!(tokens[index - 2].text.as_str(), "." | "?."))
        {
            anchors.insert(token.start, Some(token.start));
            if module.source.starts_with("/*") {
                anchors.insert(0, Some(0));
            }
        }
    }
}

/// The contextual operator and its trailing gap retain the binding endpoint.
pub(super) fn javascript_end_anchors(
    tokens: &[crate::Token],
    anchors: &mut std::collections::BTreeMap<usize, usize>,
) {
    for (name, operator) in for_of_operators(tokens) {
        anchors.insert(operator.end, name.end);
    }
}

fn for_of_operators(
    tokens: &[crate::Token],
) -> impl Iterator<Item = (&crate::Token, &crate::Token)> {
    tokens.iter().enumerate().filter_map(|(index, token)| {
        if !token.is("for") || !tokens.get(index + 1)?.is("(") {
            return None;
        }
        let first = index + 2;
        let name = first
            + usize::from(matches!(
                tokens.get(first)?.text.as_str(),
                "var" | "let" | "const"
            ));
        let (name, operator) = (tokens.get(name)?, tokens.get(name + 1)?);
        (name.kind == crate::TokenKind::Identifier && operator.is("of")).then_some((name, operator))
    })
}
