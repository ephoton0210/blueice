// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Index contracts preserve the named members and constructor capability.

use super::*;

pub(super) fn class_indexed_type(class: &ClassDeclaration, is_static: bool, object: Type) -> Type {
    let indices = class
        .members
        .iter()
        .filter_map(|member| member.index.as_ref())
        .filter(|(placement, _)| *placement == is_static)
        .map(|(_, index)| index.clone())
        .collect::<Vec<_>>();
    if indices.is_empty() {
        object
    } else {
        let indexed = Type::IndexedRecord {
            object: Box::new(object),
            indices,
        };
        if is_static {
            indexed
        } else {
            formal_class_type(class, &indexed)
        }
    }
}

impl ModuleChecker<'_> {
    pub(in crate::checker::module::binding) fn check_class_index_accesses(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) {
        if !self.checking.no_implicit_any || !self.enforce_types {
            return;
        }
        for opening in 1..tokens.len().saturating_sub(2) {
            if !tokens[opening].is("[") || !tokens[opening + 2].is("]") {
                continue;
            }
            let key = &tokens[opening + 1];
            let name = match key.kind {
                TokenKind::Number => key
                    .text
                    .parse::<f64>()
                    .ok()
                    .map(|number| number.to_string()),
                TokenKind::String => crate::syntax::string_contents(key),
                _ => None,
            };
            let Some(name) = name else {
                continue;
            };
            let Some(begin) = visibility::receiver_start(tokens, opening) else {
                continue;
            };
            let receiver = self.infer_expression(&tokens[begin..opening], scope);
            if !self.is_bound_class_instance_type(&receiver) {
                continue;
            }
            // TypeScript's literal bracket form may reach a TS private member;
            // the ordinary visibility scanner retains its existing policy.
            if self.hidden_member(&receiver, &name).is_some() {
                continue;
            }
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            if !matches!(
                property_type(
                    &receiver,
                    &name,
                    &self.types,
                    &mut HashSet::new(),
                    &mut budget
                ),
                PropertyType::Missing
            ) {
                continue;
            }
            let receiver_text = crate::diagnostic::type_text::render_in(&receiver, self.project);
            let key_text = crate::diagnostic::type_text::render(&Type::Literal(key.text.clone()));
            let span = SourceSpan::new(
                &self.module.id,
                tokens[begin].start,
                tokens[opening + 2].end,
            );
            self.typescript_type_error(
                &span,
                "class has no property at this index".into(),
                DiagnosticCode::TypeMismatch,
                7053,
                vec![key_text, receiver_text.clone()],
            );
            if let Some(counterpart) = self
                .diagnostics
                .last_mut()
                .and_then(|diagnostic| diagnostic.typescript.as_mut())
            {
                counterpart.message.push_str(&format!(
                    "\n  Property '{name}' does not exist on type '{receiver_text}'."
                ));
            }
            if let Some(diagnostic) = self.diagnostics.last_mut() {
                *diagnostic = diagnostic.clone().with_source_position(&self.module.source);
            }
        }
    }

    pub(in crate::checker::module::binding) fn bind_class_indices(&mut self) {
        let classes = self.module.classes().collect::<Vec<_>>();
        for class in classes {
            for is_static in [false, true] {
                let inherited = self.specialized_base_surface(class, is_static);
                let surface = if is_static {
                    self.values.get_mut(&class.name)
                } else {
                    self.types
                        .get_mut(&class.name)
                        .map(|definition| &mut definition.value)
                };
                let Some(surface) = surface else {
                    continue;
                };
                *surface = class_indexed_type(class, is_static, surface.clone());
                let Some(Type::IndexedRecord {
                    indices: inherited, ..
                }) = inherited
                else {
                    continue;
                };
                if let Type::IndexedRecord { indices, .. } = surface {
                    let own = indices
                        .iter()
                        .map(|index| index.key.clone())
                        .collect::<Vec<_>>();
                    indices.extend(
                        inherited
                            .into_iter()
                            .filter(|index| !own.contains(&index.key)),
                    );
                } else if !inherited.is_empty() {
                    *surface = Type::IndexedRecord {
                        object: Box::new(surface.clone()),
                        indices: inherited,
                    };
                }
            }
        }
    }

    pub(in crate::checker::module::binding) fn validate_class_indices(
        &mut self,
        class: &ClassDeclaration,
    ) {
        for is_static in [false, true] {
            let object = Type::Record(class_method_fields(class, is_static));
            let surface = class_indexed_type(class, is_static, object);
            if !matches!(surface, Type::IndexedRecord { .. }) {
                continue;
            }
            self.check_type(&surface, &class.span);
            if let Type::IndexedRecord { indices, .. } = surface {
                for index in indices {
                    self.check_type(&index.key, &index.key_span);
                    self.check_type(&index.value, &index.span);
                }
            }
        }
    }
}
