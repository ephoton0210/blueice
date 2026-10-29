// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Typed computed reads and writes preserve key domains and tuple positions.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn computed_read_type(
        &self,
        owner: &Type,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> Option<Type> {
        let key = self.infer_expression(tokens, scope);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let expanded = crate::checker::type_operators::expanded(
            owner,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        if let Type::Literal(literal) = &key {
            let name = serde_json::from_str::<String>(literal)
                .unwrap_or_else(|_| literal.trim_matches('\'').to_string());
            if let PropertyType::Found { value, .. } = property_type(
                &expanded,
                &name,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                return Some(value);
            }
        }
        if let Type::IndexedRecord { object, indices } = &expanded {
            let name = crate::symbol_keys::name(
                &self.module.id,
                &tokens
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<String>(),
            );
            if let PropertyType::Found { value, .. } =
                property_type(object, &name, &self.types, &mut HashSet::new(), &mut budget)
            {
                return Some(value);
            }
            let index = indices.iter().find(|index| {
                is_assignable(
                    &key,
                    &index.key,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                )
            })?;
            return Some(if self.checking.no_unchecked_indexed_access {
                crate::checker::type_operators::union([index.value.clone(), Type::Undefined])
            } else {
                index.value.clone()
            });
        }
        if matches!(key, Type::UniqueSymbol(_)) {
            let name = crate::symbol_keys::name(
                &self.module.id,
                &tokens
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<String>(),
            );
            if let PropertyType::Found { value, .. } = property_type(
                &expanded,
                &name,
                &self.types,
                &mut HashSet::new(),
                &mut budget,
            ) {
                return Some(value);
            }
        }
        None
    }

    pub(super) fn check_computed_assignment(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) -> bool {
        let Some((target, _, value)) = top_level_binary_parts(tokens, &["="], |_| false) else {
            return false;
        };
        let Some((receiver, _)) = member_access_target(target) else {
            return false;
        };
        if !target.last().is_some_and(|token| token.is("]")) {
            return false;
        }
        let inside = &target[receiver.len() + 1..target.len() - 1];
        let owner = self.infer_expression(receiver, scope);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        budget.checking = self.checking;
        let expanded = crate::checker::type_operators::expanded(
            &owner,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        // Readonly diagnostics are owned by the existing additional-expression path.
        let (expected, display) = match expanded {
            Type::IndexedRecord { object, indices } => {
                let key = self.infer_expression(inside, scope);
                let name = if matches!(key, Type::UniqueSymbol(_)) {
                    Some(crate::symbol_keys::name(
                        &self.module.id,
                        &inside
                            .iter()
                            .map(|token| token.text.as_str())
                            .collect::<String>(),
                    ))
                } else {
                    member_access_target(target).and_then(|(_, name)| name.map(str::to_string))
                };
                let declared = name.as_ref().and_then(|name| {
                    match property_write_type(
                        &object,
                        name,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) {
                        PropertyType::Found { value, readonly } => Some((value, readonly)),
                        _ => None,
                    }
                });
                if let Some((value, readonly)) = declared {
                    if readonly {
                        self.readonly_computed_property(inside, name.as_deref().unwrap());
                        return true;
                    }
                    (value.clone(), value)
                } else {
                    let Some(index) = indices.iter().find(|index| {
                        is_assignable(
                            &key,
                            &index.key,
                            &self.types,
                            &mut HashSet::new(),
                            &mut budget,
                        )
                    }) else {
                        return false;
                    };
                    if index.readonly {
                        return false;
                    }
                    (index.value.clone(), index.value.clone())
                }
            }
            Type::Record(_) | Type::CallableRecord { .. } => {
                let key = self.infer_expression(inside, scope);
                if !matches!(key, Type::UniqueSymbol(_)) {
                    return false;
                }
                let name = crate::symbol_keys::name(
                    &self.module.id,
                    &inside
                        .iter()
                        .map(|token| token.text.as_str())
                        .collect::<String>(),
                );
                let PropertyType::Found { value, readonly } = property_write_type(
                    &expanded,
                    &name,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) else {
                    return false;
                };
                if readonly {
                    self.readonly_computed_property(inside, &name);
                    return true;
                }
                (value.clone(), value)
            }
            Type::Tuple(elements) => {
                let Some(index) = canonical_index_key(inside) else {
                    return false;
                };
                let Ok(elements) = expand_concrete_tuple_spreads(
                    &elements,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget,
                ) else {
                    return false;
                };
                let Some(element) = elements
                    .get(index)
                    .or_else(|| elements.last().filter(|element| element.rest))
                else {
                    return false;
                };
                if !element.optional && !element.rest {
                    return false;
                }
                let display = if element.rest {
                    element.indexed_type()
                } else {
                    element.annotation.clone()
                };
                let expected = if element.optional && !self.checking.exact_optional_property_types {
                    crate::checker::type_operators::union([display.clone(), Type::Undefined])
                } else {
                    display.clone()
                };
                (expected, display)
            }
            _ => return false,
        };
        let actual = self.infer_in_context(value, scope, &expected);
        if !self.is_assignable_bounded(&actual, &expected, span) {
            let lhs = SourceSpan::new(&self.module.id, target[0].start, target.last().unwrap().end);
            self.typescript_type_error(
                &lhs,
                "computed assignment is incompatible".into(),
                DiagnosticCode::TypeMismatch,
                2322,
                vec![
                    crate::diagnostic::type_text::render_in(&actual, self.project),
                    crate::diagnostic::type_text::render_in(&display, self.project),
                ],
            );
        }
        true
    }

    fn readonly_computed_property(&mut self, inside: &[Token], name: &str) {
        let point = SourceSpan::new(&self.module.id, inside[0].start, inside.last().unwrap().end);
        self.typescript_type_error(
            &point,
            "computed property is readonly".into(),
            DiagnosticCode::TypeMismatch,
            2540,
            vec![crate::symbol_keys::display(name).unwrap_or_else(|| name.to_string())],
        );
        if let Some(diagnostic) = self.diagnostics.last_mut() {
            *diagnostic = diagnostic.clone().with_source_position(&self.module.source);
        }
    }
}
