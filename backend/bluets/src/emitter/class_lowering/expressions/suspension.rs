// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Suspended keys remain arguments in their original lexical context.
use super::*;
use crate::TokenKind;

#[derive(Default)]
pub(super) struct Capture {
    pub parameters: Vec<String>,
    pub arguments: Vec<String>,
}

fn expression(module: &Module, edits: &mut Vec<TextEdit>, from: usize, to: usize) -> String {
    let mut local = Vec::new();
    edits.retain(|edit| {
        if edit.start >= from && edit.end <= to {
            local.push(edit.clone());
            false
        } else {
            true
        }
    });
    render_span(&module.source, &local, from, to)
}

pub(super) fn capture(
    module: &Module,
    class: &mut ClassDeclaration,
    edits: &mut Vec<TextEdit>,
    tokens: &[Token],
) -> Capture {
    let mut result = Capture::default();
    if !class
        .members
        .iter()
        .flat_map(|member| &member.key)
        .any(|token| token.is("await") || token.is("yield"))
    {
        return result;
    }
    let mut fresh = |kind: &str| {
        let mut name = format!("_bts{kind}{}", class.span.start);
        while tokens.iter().any(|token| token.is(&name)) || result.parameters.contains(&name) {
            name.push('_');
        }
        result.parameters.push(name.clone());
        name
    };
    if let Some(span) = &class.extends_span {
        let name = fresh("Base");
        result.arguments.push(format!(
            "({})",
            expression(module, edits, span.start, span.end)
        ));
        edits.push(TextEdit {
            start: span.start,
            end: span.end,
            replacement: name.clone(),
        });
        class.extends_name = Some(name);
    }
    for member in &mut class.members {
        if member.key.len() < 3 || !member.key[0].is("[") {
            continue;
        }
        let name = fresh("Key");
        let first = &member.key[0];
        let last = member.key.last().expect("key close");
        let argument = expression(module, edits, first.end, last.start);
        result.arguments.push(format!("({argument})"));
        edits.push(TextEdit {
            start: first.start,
            end: last.end,
            replacement: format!("[{name}]"),
        });
        member.key = vec![
            first.clone(),
            Token {
                kind: TokenKind::Identifier,
                text: name,
                start: first.end,
                end: first.end,
            },
            last.clone(),
        ];
    }
    result
}
