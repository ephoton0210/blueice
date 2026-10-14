// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fresh object properties are checked at each contextual expression boundary.
use super::*;

struct Excess {
    token: Token,
    target: Type,
    related: Option<(TypeField, Type)>,
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_fresh_properties(
        &mut self,
        tokens: &[Token],
        expected: &Type,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let Some(excess) = self.fresh_excess(tokens, expected, scope, &mut budget, 0) else {
            return false;
        };
        self.typescript_type_error(
            span,
            "object literal has a property outside its contextual type".into(),
            DiagnosticCode::TypeMismatch,
            2353,
            vec![
                property_name(&excess.token),
                crate::diagnostic::type_text::render_in(&excess.target, self.project),
            ],
        );
        self.point_last_typescript(std::slice::from_ref(&excess.token));
        if let Some((field, parent)) = excess.related {
            let origin = SourceSpan::new(
                &field.span.module,
                field.span.start,
                field.span.start + field.name.len(),
            );
            let message = format!(
                "The expected type comes from property '{}' which is declared here on type '{}'",
                field.name,
                crate::diagnostic::type_text::render_in(&parent, self.project)
            );
            if let Some(counterpart) = self
                .diagnostics
                .last_mut()
                .and_then(|d| d.typescript.as_mut())
            {
                counterpart.related_information.push(
                    crate::diagnostic::TypeScriptRelatedInformation {
                        code: 6500,
                        message,
                        span: origin,
                        position: None,
                    },
                );
            }
        }
        true
    }

    fn fresh_excess(
        &self,
        tokens: &[Token],
        expected: &Type,
        scope: &BTreeMap<String, Type>,
        budget: &mut TypeExpansionBudget,
        depth: usize,
    ) -> Option<Excess> {
        if depth > 128 {
            return None;
        }
        let tokens = strip_outer_parentheses(tokens);
        let mut target = expected.clone();
        let mut visited = HashSet::new();
        while let Some(next) =
            instantiate_named(&target, &self.types, &mut visited, budget, "fresh context")
        {
            target = next;
        }
        if let Type::Readonly(inner) = target {
            target = *inner;
        }
        if let Some(elements) = Self::literal_elements(tokens) {
            for (index, element) in elements.iter().enumerate() {
                let item = match &target {
                    Type::Array(item) => Some(item.as_ref()),
                    Type::Tuple(items) => items.get(index).map(|item| &item.annotation),
                    _ => None,
                };
                if let Some(item) = item {
                    if let Some(excess) = self.fresh_excess(element, item, scope, budget, depth + 1)
                    {
                        return Some(excess);
                    }
                }
            }
            return None;
        }
        let members = object_members(tokens)?;
        if let Type::Union(parts) = &target {
            let mut selected = Vec::new();
            for part in parts {
                let mut expanded = part.clone();
                let mut seen = HashSet::new();
                while let Some(next) =
                    instantiate_named(&expanded, &self.types, &mut seen, budget, "fresh union")
                {
                    expanded = next;
                }
                let Type::Record(fields) = &expanded else {
                    return None;
                };
                let fits = fields
                    .iter()
                    .filter(|field| matches!(field.value, Type::Literal(_)))
                    .all(|field| {
                        members
                            .iter()
                            .find(|(key, _)| property_name(key) == field.name)
                            .is_none_or(|(_, value)| {
                                let actual = self.infer_in_context(value, scope, &field.value);
                                is_assignable(
                                    &actual,
                                    &field.value,
                                    &self.types,
                                    &mut HashSet::new(),
                                    budget,
                                )
                            })
                    });
                if fits {
                    selected.push((part.clone(), fields.clone()));
                }
            }
            for (key, _) in &members {
                let name = property_name(key);
                if !selected.is_empty()
                    && !selected
                        .iter()
                        .any(|(_, fields)| fields.iter().any(|field| field.name == name))
                {
                    return Some(Excess {
                        token: (*key).clone(),
                        target: if selected.len() == 1 {
                            selected[0].0.clone()
                        } else {
                            expected.clone()
                        },
                        related: None,
                    });
                }
            }
            return None;
        }
        let fields = match &target {
            Type::Record(fields) if fields.is_empty() => return None,
            Type::Record(fields) | Type::CallableRecord { fields, .. } => fields,
            Type::IndexedRecord { .. } => return None,
            _ => return None,
        };
        for (key, value) in members {
            let name = property_name(key);
            let Some(field) = fields.iter().find(|field| field.name == name) else {
                // Restricted class members are present under nominal markers;
                // their compatibility error belongs to the assignment relation.
                if self.hidden_member(expected, &name).is_some() {
                    continue;
                }
                return Some(Excess {
                    token: key.clone(),
                    target: expected.clone(),
                    related: None,
                });
            };
            if let Some(mut excess) =
                self.fresh_excess(value, &field.value, scope, budget, depth + 1)
            {
                if excess.related.is_none() {
                    excess.related = Some((field.clone(), expected.clone()));
                }
                return Some(excess);
            }
        }
        None
    }
}

fn property_name(token: &Token) -> String {
    crate::syntax::string_contents(token).unwrap_or_else(|| token.text.clone())
}

fn object_members(tokens: &[Token]) -> Option<Vec<(&Token, &[Token])>> {
    if !tokens.first()?.is("{") || !tokens.last()?.is("}") {
        return None;
    }
    let mut result = Vec::new();
    let mut start = 1;
    let mut depth = 0usize;
    for index in 1..tokens.len() {
        let token = &tokens[index];
        if depth == 0 && (token.is(",") || index + 1 == tokens.len()) {
            let piece = &tokens[start..index];
            if let Some(key) = piece.first().filter(|key| !key.is("...")) {
                if piece.get(1).is_some_and(|token| token.is(":")) {
                    result.push((key, &piece[2..]));
                } else if piece.len() == 1 {
                    result.push((key, piece));
                }
            }
            start = index + 1;
        } else {
            match token.text.as_str() {
                "{" | "[" | "(" => depth += 1,
                "}" | "]" | ")" => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Some(result)
}
