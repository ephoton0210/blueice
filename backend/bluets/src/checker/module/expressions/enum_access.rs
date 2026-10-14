// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Indexing an enum object: `E["A"]` reads a member and `E[0]` maps a value
//! back to its name.

use super::*;

impl ModuleChecker<'_> {
    /// The enum whose object type `value` is, if it is one.
    fn enum_object_of(&self, value: &Type) -> Option<String> {
        let Type::Named { name, .. } = value else {
            return None;
        };
        let definition = self.types.get(name)?;
        (definition.kind == TypeDefinitionKind::EnumObject)
            .then(|| name.strip_prefix("typeof ").unwrap_or(name).to_string())
    }

    /// A lone number literal read where an enum type is expected keeps its
    /// value as a literal type, so it is accepted only if some member has that
    /// value: TypeScript accepts `const e: E = 1` but not `const e: E = 5`.
    pub(in crate::checker::module) fn enum_literal_for(
        &self,
        tokens: &[Token],
        expected: &Type,
    ) -> Option<Type> {
        let Type::Named { name, .. } = expected else {
            return None;
        };
        if self.types.get(name)?.kind != TypeDefinitionKind::Enum {
            return None;
        }
        let (negative, literal) = match strip_outer_parentheses(tokens) {
            [literal] if literal.kind == TokenKind::Number => (false, literal),
            [sign, literal] if sign.is("-") && literal.kind == TokenKind::Number => (true, literal),
            _ => return None,
        };
        let value = crate::enum_eval::parse_number(&literal.text)?;
        Some(Type::Literal(crate::enum_eval::js_number_text(
            if negative { -value } else { value },
        )))
    }

    /// Whether any member of the enum is numeric, so it has a reverse mapping.
    fn enum_has_numeric_member(&self, enum_name: &str) -> bool {
        let Some(TypeDefinition {
            value: Type::Record(fields),
            ..
        }) = self.types.get(&format!("typeof {enum_name}"))
        else {
            return false;
        };
        fields.iter().any(|field| match &field.value {
            Type::Literal(member) => self
                .types
                .get(member)
                .is_some_and(|definition| {
                    definition.kind == TypeDefinitionKind::EnumMember
                        && matches!(&definition.value, Type::Literal(text) if text.parse::<f64>().is_ok())
                }),
            // A computed member has the enum's own type.
            Type::Named { .. } => true,
            _ => false,
        })
    }

    /// The type of `E[index]` for a number-like index: the member's name. `None`
    /// when `owner` is not an enum object.
    pub(in crate::checker::module) fn enum_index_type(
        &self,
        owner: &Type,
        index: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let enum_name = self.enum_object_of(owner)?;
        let index_type = self.infer_expression(index, scope);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        if self.enum_has_numeric_member(&enum_name)
            && is_assignable(
                &index_type,
                &Type::Number,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            )
        {
            Some(Type::String)
        } else {
            Some(Type::Unknown)
        }
    }

    /// Every `E[..]` on an enum object must read a member by its name, or map a
    /// number back to a name in an enum that has numeric members.
    pub(in crate::checker::module) fn check_enum_index_uses(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Identifier
                || !tokens.get(index + 1).is_some_and(|open| open.is("["))
                || index
                    .checked_sub(1)
                    .and_then(|before| tokens.get(before))
                    .is_some_and(|before| before.is("."))
            {
                continue;
            }
            let Some(value) = scope.get(&token.text) else {
                continue;
            };
            let Some(enum_name) = self.enum_object_of(value) else {
                continue;
            };
            let Some(close) = matching_bracket(tokens, index + 1) else {
                continue;
            };
            let inside = &tokens[index + 2..close];
            let access_span = SourceSpan::new(&span.module, token.start, tokens[close].end);
            if let [literal] = inside {
                if literal.kind == TokenKind::String {
                    let key = crate::enum_eval::decode_plain_string(&literal.text);
                    let known = key.as_deref().is_some_and(|key| {
                        matches!(
                            self.types.get(&format!("typeof {enum_name}")),
                            Some(TypeDefinition { value: Type::Record(fields), .. })
                                if fields.iter().any(|field| field.name == key)
                        )
                    });
                    if !known {
                        self.type_error(
                            &access_span,
                            format!("`{}` is not a member of enum `{enum_name}`", literal.text),
                            DiagnosticCode::TypeMismatch,
                        );
                    }
                    continue;
                }
            }
            // A const enum has no object to map a number back through; the
            // use is reported by the const enum checks.
            if self.const_enums.contains(&token.text) {
                continue;
            }
            let index_type = self.infer_expression(inside, scope);
            let numeric = self.is_assignable_bounded(&index_type, &Type::Number, span);
            if !(numeric && self.enum_has_numeric_member(&enum_name)) {
                self.type_error(
                    &access_span,
                    format!(
                        "an index of type `{}` cannot be used on enum `{enum_name}`",
                        type_label(&index_type)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }
}

/// The index of the `]` closing the `[` at `open`.
fn matching_bracket(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.is("[") {
            depth += 1;
        } else if token.is("]") {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}
