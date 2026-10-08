// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reuse class parsing without declaring an expression's self name in its parent.

use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn try_parse_class_expression(
        &mut self,
        index: usize,
        end: usize,
    ) -> Option<usize> {
        if !self.tokens[index].is("class")
            || !index.checked_sub(1).is_some_and(|before| {
                matches!(
                    self.tokens[before].text.as_str(),
                    "=" | "return" | "throw" | "(" | "[" | "," | ":" | "?" | "=>"
                )
            })
        {
            return None;
        }
        let start = self.tokens[index].start;
        if let Some(expression) = self.class_expressions.get(&start) {
            return self.tokens[index..end]
                .iter()
                .position(|token| token.end == expression.class.span.end)
                .map(|relative| index + relative + 1);
        }
        let next = self.tokens.get(index + 1)?;
        let named = next.kind == TokenKind::Identifier;
        if !named && !matches!(next.text.as_str(), "{" | "<" | "extends") {
            return None;
        }
        let name = named.then(|| next.text.clone());
        let saved_index = self.index;
        let saved_decorators = std::mem::take(&mut self.pending_decorators);
        let identity = Token {
            kind: TokenKind::Identifier,
            text: format!("#class@{start}"),
            start: next.start,
            end: if named { next.end } else { next.start },
        };
        let original_name = if named {
            Some(std::mem::replace(&mut self.tokens[index + 1], identity))
        } else {
            self.tokens.insert(index + 1, identity);
            None
        };
        self.index = index + 1;
        let declarations = self.declarations.len();
        self.parse_class(start, false, None);
        let next_index = self.index - usize::from(!named);
        if let Some(original) = original_name {
            self.tokens[index + 1] = original;
        } else {
            self.tokens.remove(index + 1);
        }
        self.index = saved_index;
        self.pending_decorators = saved_decorators;
        if self.declarations.len() > declarations {
            if let Declaration::Class(class) = self.declarations.pop().unwrap() {
                self.class_expressions
                    .insert(start, ClassExpression { name, class });
            }
        }
        Some(next_index.min(end))
    }
}
