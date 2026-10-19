// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Statement diagnostics/erasure also apply outside structured function bodies.

use super::*;

impl Parser {
    pub(in crate::parser::implementation) fn diagnose_throw_operand(&mut self, index: usize) {
        let keyword = &self.tokens[index];
        let next = &self.tokens[index + 1];
        if self.source[keyword.end..next.start].contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    SourceSpan::new(&self.id, keyword.end, keyword.end),
                    "a line terminator is not permitted after `throw`",
                )
                .with_typescript(1142, Vec::new()),
            );
        } else if next.is(";") || next.is("}") || next.kind == TokenKind::Eof {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    next.span(&self.id),
                    "expected an expression after `throw`",
                )
                .with_typescript(1109, Vec::new()),
            );
        }
    }

    pub(in crate::parser::implementation) fn collect_catch_annotation(
        &mut self,
        index: usize,
        end: usize,
    ) -> Option<usize> {
        if !self.tokens[index].is("catch")
            || !self.catch_follows_try_block(index)
            || !self.tokens.get(index + 1)?.is("(")
            || self.tokens.get(index + 2)?.kind != TokenKind::Identifier
            || !self.tokens.get(index + 3)?.is(":")
            || index + 4 >= end
        {
            return None;
        }
        let saved = self.index;
        self.index = index + 4;
        let start = self.current().start;
        let value = self.parse_type_until(&[")"]);
        let next = self.index;
        let finish = self.previous().end;
        self.index = saved;
        self.edits.push(TextEdit {
            start: self.tokens[index + 3].start,
            end: finish,
            replacement: String::new(),
        });
        if !matches!(value, Type::Any | Type::Unknown | Type::StrictUnknown) {
            self.diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::ParseError,
                    SourceSpan::new(&self.id, start, finish),
                    "a catch clause variable type annotation must be `any` or `unknown`",
                )
                .with_typescript(1196, Vec::new()),
            );
        }
        Some(next)
    }

    fn catch_follows_try_block(&self, index: usize) -> bool {
        if index == 0 || !self.tokens[index - 1].is("}") {
            return false;
        }
        let mut depth = 0usize;
        for open in (0..index).rev() {
            if self.tokens[open].is("}") {
                depth += 1;
            } else if self.tokens[open].is("{") {
                depth -= 1;
                if depth == 0 {
                    return open > 0 && self.tokens[open - 1].is("try");
                }
            }
        }
        false
    }

    pub(in crate::parser::implementation) fn diagnose_missing_namespace_name(&mut self) {
        let keyword = self.current();
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::UnknownName,
                keyword.span(&self.id),
                format!("cannot find value `{}`", keyword.text),
            )
            .with_typescript(2304, vec![keyword.text.clone()]),
        );
        self.diagnostics.push(
            Diagnostic::error(
                DiagnosticCode::ParseError,
                self.tokens[self.index + 1].span(&self.id),
                "expected a namespace name",
            )
            .with_typescript(1437, Vec::new()),
        );
    }
}
