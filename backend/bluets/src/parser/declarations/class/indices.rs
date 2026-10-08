// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class index signatures are contracts, with no runtime field or binding.

use super::*;

impl Parser {
    pub(super) fn parse_class_index(
        &mut self,
        start: usize,
        member: &mut ClassMemberShell,
    ) -> bool {
        let end = start + member.token_end - member.token_start;
        let Some(modifiers) = scan_member_modifiers(&self.tokens, start, end) else {
            return false;
        };
        let opening = modifiers.name_index;
        if !self.tokens[opening].is("[")
            || !self
                .tokens
                .get(opening + 2)
                .is_some_and(|token| token.is(":") || token.is("?"))
        {
            return false;
        }
        self.index = opening + 1;
        let mut parameters = Vec::new();
        loop {
            let name_token = self.current().clone();
            let name = self.require_identifier("expected a class index parameter name");
            if self.consume("?") {
                self.diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::ParseError,
                        self.previous().span(&self.id),
                        "index parameters cannot be optional",
                    )
                    .with_typescript(1019, Vec::new()),
                );
            }
            self.expect(":");
            let key_start = self.current().start;
            let key = self.parse_type_until(&[",", "]"]);
            let key_span = SourceSpan::new(&self.id, key_start, self.previous().end);
            parameters.push((name, name_token.span(&self.id), key, key_span));
            if !self.consume(",") {
                break;
            }
        }
        self.expect("]");
        self.expect(":");
        let value = self.parse_type_until(&[";"]);
        self.consume(";");
        if parameters.len() != 1 {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    parameters[0].1.clone(),
                    "an index signature requires one parameter",
                )
                .with_typescript(1096, Vec::new()),
            );
        }
        let (name, _, key, key_span) = parameters.remove(0);
        member.index = Some((
            modifiers.is_static,
            IndexSignature {
                name,
                key,
                value,
                readonly: modifiers.readonly_token.is_some(),
                key_span,
                span: SourceSpan::new(&self.id, self.tokens[opening].start, self.previous().end),
            },
        ));
        true
    }
}
