// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Template operands are lexical subgraphs, not one opaque source-map token.
use super::*;

pub(super) fn anchors(
    module: &Module,
    tokens: &[crate::Token],
    anchors: &mut BTreeMap<usize, Option<usize>>,
) {
    visit(module, tokens, anchors, 128);
}

fn visit(
    module: &Module,
    tokens: &[crate::Token],
    anchors: &mut BTreeMap<usize, Option<usize>>,
    depth: usize,
) {
    if depth == 0 {
        return;
    }
    for token in tokens
        .iter()
        .filter(|token| token.kind == crate::TokenKind::Template)
    {
        let bytes = module.source.as_bytes();
        let mut at = token.start + 1;
        while at + 1 < token.end {
            if bytes[at] == b'\\' {
                at += 2;
                continue;
            }
            if !bytes[at..].starts_with(b"${") {
                at += 1;
                continue;
            }
            let Ok((close, expression)) =
                crate::syntax::lex_expression(&module.id, &module.source, at + 2)
            else {
                break;
            };
            for (index, item) in expression.iter().enumerate() {
                if item.kind == crate::TokenKind::Eof {
                    continue;
                }
                if token_maps::mark_start(&module.source, &expression, index) {
                    anchors.insert(item.start, Some(item.start));
                }
                if token_maps::mark_end(&expression, index) {
                    anchors.insert(item.end, Some(item.end));
                }
            }
            anchors.insert(close, Some(close));
            visit(module, &expression, anchors, depth - 1);
            at = close + 1;
        }
    }
}
