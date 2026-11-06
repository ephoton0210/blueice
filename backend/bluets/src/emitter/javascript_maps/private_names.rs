// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A retained private identifier is one original name, including its `#`.
use super::*;
use crate::parser::ClassDeclaration;

fn classes(module: &Module) -> Vec<&ClassDeclaration> {
    runtime_declarations(&module.declarations)
        .into_iter()
        .filter_map(|(declaration, _)| {
            if let Declaration::Class(class) = declaration {
                Some(class)
            } else {
                None
            }
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| &expression.class),
        )
        .collect()
}

pub(super) fn anchors(
    module: &Module,
    options: &CompilerOptions,
    anchors: &mut BTreeMap<usize, Option<usize>>,
) {
    private_names(module, options, |token, operator| {
        anchors.insert(token.start, Some(token.start));
        anchors.insert(token.start + 1, Some(token.start));
        if let Some(operator) = operator {
            anchors.insert(operator.start, Some(token.end));
        }
    });
}

pub(super) fn end_anchors(
    module: &Module,
    options: &CompilerOptions,
    anchors: &mut BTreeMap<usize, usize>,
) {
    private_names(module, options, |token, operator| {
        if let Some(operator) = operator {
            anchors.insert(operator.end, token.end);
        }
    });
}

fn private_names(
    module: &Module,
    options: &CompilerOptions,
    mut visit: impl FnMut(&crate::Token, Option<&crate::Token>),
) {
    if options.target < crate::EcmaTarget::Es2022 {
        return;
    }
    for class in classes(module) {
        let names = class
            .members
            .iter()
            .flat_map(|member| member.key.iter())
            .filter(|token| token.text.starts_with('#'))
            .map(|token| token.text.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for (index, token) in class.body.iter().enumerate() {
            if !names.contains(token.text.as_str()) || !token.text.starts_with('#') {
                continue;
            }
            visit(
                token,
                class
                    .body
                    .get(index + 1)
                    .filter(|operator| operator.is("in")),
            );
        }
    }
}
