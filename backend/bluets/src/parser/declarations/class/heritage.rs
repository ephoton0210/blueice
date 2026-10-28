// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A named computed base is evaluated once and retains a constructor alias.
use super::*;

impl Parser {
    pub(super) fn computed_heritage_ahead(&self) -> bool {
        if self.peek("(") {
            return true;
        }
        let mut index = self.index + 1;
        while self.tokens.get(index).is_some_and(|token| token.is("."))
            && self
                .tokens
                .get(index + 1)
                .is_some_and(|token| token.kind == TokenKind::Identifier)
        {
            index += 2;
        }
        self.tokens
            .get(index)
            .is_some_and(|token| token.is("(") || token.is("["))
    }

    pub(super) fn parse_computed_heritage(
        &mut self,
        class_name: &str,
        class_start: usize,
    ) -> Option<Token> {
        if class_name.starts_with("#class@") {
            self.unsupported(self.current().span(&self.id),"a computed class-expression heritage requires a supported lexical constructor context");
            return None;
        }
        let begin = self.index;
        let mut end = begin;
        let mut stack = Vec::new();
        while end < self.tokens.len() - 1 {
            let text = self.tokens[end].text.as_str();
            if stack.is_empty() && matches!(text, "{" | "implements") {
                break;
            }
            match text {
                "(" => stack.push(")"),
                "[" => stack.push("]"),
                "{" => stack.push("}"),
                ")" | "]" | "}" if stack.last().copied() == Some(text) => {
                    stack.pop();
                }
                _ => {}
            }
            end += 1;
        }
        if end == begin || end >= self.tokens.len() - 1 {
            self.error_here(
                DiagnosticCode::ParseError,
                "expected a computed class heritage expression",
            );
            return None;
        }
        let from = self.tokens[begin].start;
        let to = self.tokens[end - 1].end;
        let mut name = format!("{class_name}_base");
        let mut suffix = 1;
        while self.tokens.iter().any(|token| token.is(&name)) {
            name = format!("{class_name}_base_{suffix}");
            suffix += 1;
        }
        let initializer = self.tokens[begin..end].to_vec();
        self.collect_expression_type_edits(begin, end);
        // Emit moves this expression after import-use analysis and module name
        // rewriting, so its value references keep their original identities.
        self.declarations
            .push(Declaration::Variable(VariableDeclaration {
                name: name.clone(),
                pattern: None,
                kind: VariableKind::Const,
                annotation: None,
                initializer,
                exported: false,
                declared: false,
                span: SourceSpan::new(&self.id, class_start, class_start),
            }));
        self.index = end;
        Some(Token {
            kind: TokenKind::Identifier,
            text: name,
            start: from,
            end: to,
        })
    }
}
