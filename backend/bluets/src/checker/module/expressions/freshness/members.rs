// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Freshness tracks written keys, never the properties introduced by a spread.
use super::*;

pub(super) struct Member<'a> {
    pub token: Token,
    pub name: String,
    pub value: &'a [Token],
}

impl ModuleChecker<'_> {
    pub(super) fn fresh_members<'a>(
        &self,
        tokens: &'a [Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Vec<Member<'a>>> {
        if !tokens.first()?.is("{") || !tokens.last()?.is("}") {
            return None;
        }
        let mut result = Vec::new();
        for piece in split_members(&tokens[1..tokens.len() - 1]) {
            let Some(key) = piece.first().filter(|key| !key.is("...")) else {
                continue;
            };
            let (name, token, end) = if key.is("[") {
                let close = crate::checker::scopes::targets::close(piece, 0)?;
                let operand = &piece[1..close];
                let key_type = if let [key] = operand {
                    if matches!(key.kind, TokenKind::String | TokenKind::Number) {
                        Type::Literal(key.text.clone())
                    } else {
                        self.infer_expression(operand, scope)
                    }
                } else {
                    self.infer_expression(operand, scope)
                };
                let name = match key_type {
                    Type::Literal(text) => text.trim_matches(['\'', '"']).to_string(),
                    Type::UniqueSymbol(_) => crate::symbol_keys::name(
                        &self.module.id,
                        &operand.iter().map(|t| t.text.as_str()).collect::<String>(),
                    ),
                    _ => continue,
                };
                let token = Token {
                    kind: TokenKind::Identifier,
                    text: self.module.source[key.start..piece[close].end].to_string(),
                    start: key.start,
                    end: piece[close].end,
                };
                (name, token, close)
            } else {
                let name = crate::syntax::string_contents(key).unwrap_or_else(|| key.text.clone());
                let mut token = key.clone();
                token.text = name.clone();
                (name, token, 0)
            };
            let value = if piece.get(end + 1).is_some_and(|t| t.is(":")) {
                &piece[end + 2..]
            } else if piece.len() == 1 {
                piece
            } else {
                continue;
            };
            result.push(Member { name, token, value });
        }
        Some(result)
    }

    pub(super) fn conditional_fresh_error(
        &mut self,
        tokens: &[Token],
        expected: &Type,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
        excess: &Excess,
        conditional_code: u32,
    ) {
        let (_, yes, no) = conditional_expression_parts(strip_outer_parentheses(tokens)).unwrap();
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let branch = if self
            .fresh_excess(yes, expected, scope, &mut budget, 0)
            .is_some()
        {
            yes
        } else {
            no
        };
        let actual = Type::Union(vec![
            self.fresh_diagnostic_type(yes, scope, 0),
            self.fresh_diagnostic_type(no, scope, 0),
        ]);
        let branch = self.fresh_diagnostic_type(branch, scope, 0);
        let text = |t: &Type| crate::diagnostic::type_text::render_in(t, self.project);
        self.typescript_type_error(
            span,
            "conditional literal exceeds its contextual type".into(),
            DiagnosticCode::TypeMismatch,
            conditional_code,
            vec![text(&actual), text(expected)],
        );
        self.point_last_typescript(std::slice::from_ref(&excess.token));
        if let Some(d) = self
            .diagnostics
            .last_mut()
            .and_then(|d| d.typescript.as_mut())
        {
            d.message.push_str(&format!(
                "\n  Type '{}' is not assignable to type '{}'.",
                text(&branch),
                text(expected)
            ));
            for (i, name) in excess.path.iter().enumerate() {
                d.message.push_str(&format!(
                    "\n{}Types of property '{name}' are incompatible.",
                    "  ".repeat(i + 2)
                ));
            }
            d.message.push_str(&format!("\n{}Object literal may only specify known properties, and '{}' does not exist in type '{}'.",
                "  ".repeat(excess.path.len()+2),excess.token.text,text(&excess.target)));
        }
    }

    fn fresh_diagnostic_type(
        &self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        depth: usize,
    ) -> Type {
        let mut actual = self.infer_expression(tokens, scope);
        if depth >= 128 {
            return actual;
        }
        if let (Type::Record(fields), Some(members)) =
            (&mut actual, self.fresh_members(tokens, scope))
        {
            // Native literal diagnostics present own fields before spread fields.
            fields.sort_by_key(|field| !members.iter().any(|m| m.name == field.name));
            for field in fields {
                if let Some(member) = members.iter().find(|m| m.name == field.name) {
                    field.value = self.fresh_diagnostic_type(member.value, scope, depth + 1);
                }
            }
        }
        actual
    }
}

fn split_members(tokens: &[Token]) -> Vec<&[Token]> {
    let mut pieces = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, t) in tokens.iter().enumerate() {
        if depth == 0 && t.is(",") {
            pieces.push(&tokens[start..i]);
            start = i + 1;
            continue;
        }
        match t.text.as_str() {
            "{" | "[" | "(" => depth += 1,
            "}" | "]" | ")" => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    pieces.push(&tokens[start..]);
    pieces
}
