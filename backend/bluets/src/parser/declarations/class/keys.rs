// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Property-name boundaries keep static identity apart from runtime evaluation.

use super::*;

pub(super) fn class_key_end(tokens: &[Token], start: usize, end: usize) -> Option<usize> {
    let token = tokens.get(start)?;
    if matches!(
        token.kind,
        TokenKind::Identifier | TokenKind::Keyword | TokenKind::String | TokenKind::Number
    ) {
        Some(start + 1)
    } else if token.is("[") {
        matching_closing_delimiter(tokens, start, end, "[", "]").map(|closing| closing + 1)
    } else {
        None
    }
}

fn literal_key(token: &Token) -> Option<String> {
    match token.kind {
        TokenKind::String => crate::syntax::string_contents(token),
        TokenKind::Number => token
            .text
            .parse::<f64>()
            .ok()
            .map(|value| value.to_string()),
        _ => None,
    }
}

impl Parser {
    pub(super) fn class_member_key(
        &mut self,
        start: usize,
        end: usize,
        member: &mut ClassMemberShell,
    ) -> (String, SourceSpan) {
        let tokens = self.tokens[start..end].to_vec();
        let span = SourceSpan::new(
            &self.id,
            tokens[0].start,
            tokens.last().expect("property name has tokens").end,
        );
        let name = match tokens.as_slice() {
            [token] => literal_key(token).unwrap_or_else(|| token.text.clone()),
            [_, token, _] => literal_key(token)
                .or_else(|| {
                    self.declarations.iter().rev().find_map(|declaration| {
                        let Declaration::Variable(variable) = declaration else {
                            return None;
                        };
                        if variable.name != token.text || variable.kind != VariableKind::Const {
                            return None;
                        }
                        let [initializer] = variable.initializer.as_slice() else {
                            return None;
                        };
                        literal_key(initializer)
                    })
                })
                .unwrap_or_else(|| format!("[computed@{}]", span.start)),
            _ => format!("[computed@{}]", span.start),
        };
        if tokens[0].is("[") {
            self.collect_expression_type_edits(start + 1, end - 1);
        }
        member.key = tokens;
        (name, span)
    }
}
