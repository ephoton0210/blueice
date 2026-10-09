// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Retained attributes share original spans with graph loading and erasure.

use super::*;

impl Parser {
    pub(super) fn parse_import_attributes(&mut self, type_only: bool) -> Option<ImportAttributes> {
        if !self.peek("with") && !self.peek("assert") {
            return None;
        }
        let start = self.current().start;
        let assertion = self.current().is("assert");
        self.bump();
        self.expect("{");
        let mut entries = Vec::new();
        while !self.at_eof() && !self.consume("}") {
            let key = if self.current().kind == TokenKind::String {
                let key = string_contents(self.current()).unwrap_or_default();
                self.bump();
                key
            } else {
                self.require_identifier("expected import attribute key")
            };
            self.expect(":");
            let value = self.current().clone();
            self.bump();
            entries.push(ImportAttribute { key, value });
            if !self.consume(",") {
                self.expect("}");
                break;
            }
        }
        let attributes = ImportAttributes {
            assertion,
            entries,
            span: SourceSpan::new(&self.id, start, self.previous().end),
        };
        if type_only {
            let mode = attributes.entries.iter().any(|entry| {
                entry.key == "resolution-mode"
                    && string_contents(&entry.value)
                        .is_some_and(|value| matches!(value.as_str(), "import" | "require"))
            });
            let code = if !mode {
                Some(2857)
            } else if attributes.entries.len() != 1 {
                Some(1464)
            } else {
                None
            };
            if let Some(code) = code {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParseError,
                        attributes.span.clone(),
                        "invalid attributes on a type-only module declaration",
                    )
                    .with_typescript(code, Vec::new()),
                );
            }
        }
        Some(attributes)
    }
}
