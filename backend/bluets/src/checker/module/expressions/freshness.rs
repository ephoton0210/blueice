// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fresh object properties are checked at each contextual expression boundary.
use super::*;
mod members;

struct Excess {
    token: Token,
    target: Type,
    related: Option<(TypeField, Type)>,
    path: Vec<String>,
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module) fn check_fresh_properties(
        &mut self,
        tokens: &[Token],
        expected: &Type,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        self.check_fresh_boundary(tokens, expected, scope, span, 2322)
    }

    pub(in crate::checker::module) fn check_fresh_return_properties(
        &mut self,
        tokens: &[Token],
        expected: &Type,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        self.check_fresh_boundary(tokens, expected, scope, span, 2353)
    }

    pub(in crate::checker::module) fn check_fresh_argument_properties(
        &mut self,
        tokens: &[Token],
        expected: &Type,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        self.check_fresh_boundary(tokens, expected, scope, span, 2345)
    }

    fn check_fresh_boundary(
        &mut self,
        tokens: &[Token],
        expected: &Type,
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
        conditional_code: u32,
    ) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let Some(excess) = self.fresh_excess(tokens, expected, scope, &mut budget, 0) else {
            return false;
        };
        if conditional_code != 2353
            && conditional_expression_parts(strip_outer_parentheses(tokens)).is_some()
        {
            self.conditional_fresh_error(tokens, expected, scope, span, &excess, conditional_code);
            return true;
        }
        self.typescript_type_error(
            span,
            "object literal has a property outside its contextual type".into(),
            DiagnosticCode::TypeMismatch,
            2353,
            vec![
                excess.token.text.clone(),
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
        if let Some((_, yes, no)) = conditional_expression_parts(tokens) {
            return self
                .fresh_excess(yes, expected, scope, budget, depth + 1)
                .or_else(|| self.fresh_excess(no, expected, scope, budget, depth + 1));
        }
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
        let members = self.fresh_members(tokens, scope)?;
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
                            .find(|member| member.name == field.name)
                            .is_none_or(|member| {
                                let actual =
                                    self.infer_in_context(member.value, scope, &field.value);
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
            for member in &members {
                let name = &member.name;
                if !selected.is_empty()
                    && !selected
                        .iter()
                        .any(|(_, fields)| fields.iter().any(|field| &field.name == name))
                {
                    return Some(Excess {
                        token: member.token.clone(),
                        target: if selected.len() == 1 {
                            selected[0].0.clone()
                        } else {
                            expected.clone()
                        },
                        related: None,
                        path: Vec::new(),
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
        for member in members {
            let name = member.name;
            let Some(field) = fields
                .iter()
                .find(|field| crate::checker::computed_keys::same(&field.name, &name, &self.types))
            else {
                // Restricted class members are present under nominal markers;
                // their compatibility error belongs to the assignment relation.
                if self.hidden_member(expected, &name).is_some() {
                    continue;
                }
                return Some(Excess {
                    token: member.token,
                    target: expected.clone(),
                    related: None,
                    path: Vec::new(),
                });
            };
            if let Some(mut excess) =
                self.fresh_excess(member.value, &field.value, scope, budget, depth + 1)
            {
                if excess.related.is_none() {
                    excess.related = Some((field.clone(), expected.clone()));
                }
                excess.path.insert(0, field.name.clone());
                return Some(excess);
            }
        }
        None
    }
}
