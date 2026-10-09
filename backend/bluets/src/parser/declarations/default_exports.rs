// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Default declaration shells and expression snapshots reuse ordinary checking.

use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn parse_default_declaration(&mut self, start: usize) {
        let async_function = self.peek("async")
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.is("function"));
        if async_function {
            self.bump();
        }
        if self.consume("function") {
            let name_index = self.index + usize::from(self.peek("*"));
            let anonymous = self.tokens[name_index].kind != TokenKind::Identifier;
            if anonymous {
                self.insert_default_name(name_index);
            }
            self.parse_function(start, true, true, false, async_function);
            if anonymous {
                self.tokens.remove(name_index);
                self.index -= 1;
                if let Some(Declaration::Function(function)) = self.declarations.last_mut() {
                    function.anonymous = true;
                }
            }
        } else if self.consume("class") {
            let name_index = self.index;
            let anonymous = self.current().kind != TokenKind::Identifier;
            if anonymous {
                self.insert_default_name(name_index);
            }
            self.parse_class(start, true, None);
            if anonymous {
                self.tokens.remove(name_index);
                self.index -= 1;
            }
            if let Some(Declaration::Class(class)) = self.declarations.last_mut() {
                class.default_export = true;
                class.anonymous = anonymous;
            }
        } else if self.consume("interface") {
            self.parse_interface(start, true);
            if let Some(Declaration::Interface(interface)) = self.declarations.last_mut() {
                interface.default_export = true;
            }
        } else if self.current().kind == TokenKind::Identifier
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.is(";") || token.kind == TokenKind::Eof)
        {
            self.parse_default_export(start);
        } else {
            let initializer_start = self.index;
            let name = self.default_internal_name("_default");
            let initializer = self.collect_until_statement_end();
            self.collect_expression_type_edits(initializer_start, self.index);
            self.collect_following_variable_types(initializer_start, self.index);
            self.consume(";");
            let span = SourceSpan::new(&self.id, start, self.previous().end);
            self.declarations
                .push(Declaration::Variable(VariableDeclaration {
                    name: name.clone(),
                    kind: VariableKind::Const,
                    annotation: None,
                    initializer,
                    exported: false,
                    declared: false,
                    span: span.clone(),
                }));
            self.declarations
                .push(Declaration::DefaultExport(DefaultExportDeclaration {
                    name,
                    expression: true,
                    span,
                }));
        }
    }

    fn default_internal_name(&self, base: &str) -> String {
        let mut name = base.to_string();
        let mut index = 1;
        while self.tokens.iter().any(|token| token.text == name)
            || self.declarations.iter().any(|declaration| matches!(declaration, Declaration::Variable(variable) if variable.name == name)) {
            name = format!("{base}_{index}");
            index += 1;
        }
        name
    }

    fn insert_default_name(&mut self, index: usize) {
        let start = self.tokens[index].start;
        let name = self.default_internal_name("default_1");
        self.tokens.insert(
            index,
            Token {
                kind: TokenKind::Identifier,
                text: name,
                start,
                end: start,
            },
        );
    }
}
