// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Recover statement continuations after refusing a reserved variable name.
use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn recover_reserved_variable(
        &mut self,
        start: usize,
        exported: bool,
        declared: bool,
        kind: VariableKind,
    ) -> Option<VariableDeclaration> {
        let name = self.current().clone();
        if name.kind != TokenKind::Keyword
            || name.is_binding_identifier()
            || !self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.is(":"))
        {
            return None;
        }
        let (code, arguments) = match name.text.as_str() {
            "class" => (1005, vec!["{".into()]),
            "return" | "throw" => (1109, Vec::new()),
            "true" | "false" | "null" | "this" => (1005, vec![";".into()]),
            _ => return None,
        };
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ParseError,
                name.span(&self.id),
                "reserved word cannot name a variable",
            )
            .with_typescript(1389, vec![name.text]),
        );
        self.bump();
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ParseError,
                self.current().span(&self.id),
                "invalid statement continuation",
            )
            .with_typescript(code, arguments),
        );
        self.bump();
        if !is_javascript(&self.id)
            && matches!(
                self.current().text.as_str(),
                "any"
                    | "unknown"
                    | "never"
                    | "number"
                    | "string"
                    | "boolean"
                    | "bigint"
                    | "symbol"
                    | "object"
            )
        {
            self.recovered_primitive_values.push(self.current().clone());
        }
        let end =
            find_balanced_delimiter(&self.tokens, self.index, self.tokens.len() - 1, &[";", "}"]);
        self.index = end;
        self.consume(";");
        Some(VariableDeclaration {
            name: "<error>".into(),
            pattern: None,
            kind,
            annotation: None,
            initializer: Vec::new(),
            exported,
            declared,
            span: SourceSpan::new(&self.id, start, self.previous().end),
        })
    }
}
