// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original type nodes retain boundaries; inferred/synthetic types have none.
use super::*;

impl Mapper<'_> {
    pub(super) fn ty(&mut self, g: &[Token], s: &[Token]) {
        if g.is_empty() || s.is_empty() {
            return;
        }
        self.point(g[0].start, s[0].start);
        let mut cursor = 0;
        for (index, token) in g.iter().enumerate() {
            if token.is(";") {
                if s.get(cursor).is_some_and(|t| t.is(";")) {
                    self.point(token.end, s[cursor].end);
                    cursor += 1;
                } else if let Some(previous) = cursor.checked_sub(1).and_then(|index| s.get(index))
                {
                    self.point(token.end, previous.end);
                }
                continue;
            }
            let node = matches!(
                token.kind,
                TokenKind::Identifier | TokenKind::String | TokenKind::Number
            ) || matches!(
                token.text.as_str(),
                "number"
                    | "string"
                    | "boolean"
                    | "void"
                    | "unknown"
                    | "any"
                    | "never"
                    | "undefined"
                    | "null"
                    | "true"
                    | "false"
                    | "object"
                    | "symbol"
                    | "bigint"
            );
            let modifier =
                token.is("readonly") && g.get(index + 2).is_some_and(|t| t.is(":") || t.is("?"));
            let punctuation = matches!(
                token.text.as_str(),
                "{" | "}" | "[" | "]" | "<" | ">" | "(" | ")" | ";" | "?" | "..."
            );
            if !node && !modifier && !punctuation {
                continue;
            }
            let Some(next) = s[cursor..].iter().position(|source| {
                source.text == token.text
                    || (source.kind == TokenKind::String
                        && token.kind == TokenKind::String
                        && crate::syntax::string_contents(source)
                            == crate::syntax::string_contents(token))
            }) else {
                continue;
            };
            cursor += next;
            let original = &s[cursor];
            if node || modifier || token.is("?") || token.is("...") {
                self.pair(token, original);
            } else if matches!(token.text.as_str(), "{" | "(") {
                self.point(token.start, original.start);
            } else if matches!(token.text.as_str(), "}" | "]" | ">" | ")" | ";") {
                self.point(token.end, original.end);
            }
            cursor += 1;
        }
        self.point(g.last().unwrap().end, s.last().unwrap().end);
    }

    pub(super) fn type_parameters(
        &mut self,
        g: &[crate::parser::TypeParameter],
        s: &[crate::parser::TypeParameter],
    ) {
        for (g, s) in g.iter().zip(s) {
            let gt = self.generated(&g.span);
            let st = self.source(&s.span);
            self.ty(&gt, &st);
        }
    }
    pub(super) fn parameters(&mut self, g: &[Parameter], s: &[Parameter]) {
        for (g, s) in g.iter().zip(s) {
            let gt = self.generated(&g.span);
            let st = self.source(&s.span);
            self.parameter(&gt, &st, g, s);
        }
    }
    pub(super) fn parameter(&mut self, g: &[Token], s: &[Token], gp: &Parameter, sp: &Parameter) {
        if g.is_empty() || s.is_empty() {
            return;
        }
        self.point(g[0].start, s[0].start);
        self.named(g, s, &gp.name);
        for special in ["...", "?"] {
            if let Some((g, s)) = g
                .iter()
                .find(|t| t.is(special))
                .zip(s.iter().find(|t| t.is(special)))
            {
                self.pair(g, s);
            }
        }
        if sp.annotation.is_some() {
            self.ty(annotation(g), annotation(s));
        }
        self.point(gp.span.end, sp.span.end);
    }
}
