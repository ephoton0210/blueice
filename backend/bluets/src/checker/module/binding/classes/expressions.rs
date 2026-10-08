// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class expressions share class validation while keeping lexical self names local.

use super::*;

impl ModuleChecker<'_> {
    fn expression_self_query(&self, identity: &str, name: &str, position: usize) -> bool {
        self.scopes
            .as_ref()
            .and_then(|scopes| scopes.query_type(name, position))
            .is_some_and(|value| {
                matches!(value.object_type(), Type::CallableRecord { fields, .. }
                if fields.iter().any(|field| field.name == "prototype"
                    && matches!(&field.value, Type::Named { name, .. } if name == identity)))
            })
    }
    pub(in crate::checker) fn class_expression_surfaces(
        &self,
    ) -> BTreeMap<String, ClassExpressionSurface> {
        self.module
            .class_expressions
            .values()
            .filter_map(|expression| {
                let name = &expression.class.name;
                let definition = self.types.get(name)?;
                Some((
                    name.clone(),
                    ClassExpressionSurface {
                        parameters: definition.parameters.clone(),
                        instance: definition.value.clone(),
                        constructor: self.values.get(name)?.clone(),
                    },
                ))
            })
            .collect()
    }

    pub(in crate::checker::module::binding) fn check_class_structure(
        &mut self,
        original: &ClassDeclaration,
    ) {
        let inferred = self.class_with_inferred_returns(original);
        let class = &inferred;
        self.with_class_type_scope(&class.instance_parameters(), |checker| {
            checker.validate_computed_class_this(class);
            checker.validate_computed_class_fields(class);
            checker.validate_class_heritage_name(class);
            checker.validate_class_heritage_arguments(class);
            checker.validate_static_class_parameters(class);
            checker.validate_class_indices(class);
            checker.validate_class_modifiers(class);
            checker.validate_class_implements(class);
            checker.validate_class_constructor_group(class);
            checker.validate_class_method_groups(class);
            checker.validate_class_method_overrides(class);
            checker.validate_class_accessors(class);
            checker.validate_class_visibility(class);
            checker.with_class_access(class, |checker| {
                checker.validate_class_fields(class);
                checker.check_class_constructor_bodies(class);
                checker.check_class_method_bodies(class);
                checker.check_class_static_blocks(class);
                checker.check_class_decorators(class);
            });
        });
    }

    pub(in crate::checker::module::binding) fn bind_class_expression_names(&mut self) {
        for expression in self.module.class_expressions.values() {
            let Some(name) = &expression.name else {
                continue;
            };
            let identity = &expression.class.name;
            let mut rename = BTreeMap::from([
                (name.clone(), identity.clone()),
                (format!("typeof {name}"), format!("typeof {identity}")),
            ]);
            for reference in &self.module.type_references {
                if reference.value_query
                    && reference.name == *name
                    && reference.span.start >= expression.class.span.start
                    && reference.span.end <= expression.class.span.end
                    && self.expression_self_query(identity, name, reference.span.start)
                {
                    rename.insert(
                        format!("typeof {name}@{}", reference.span.start),
                        format!("typeof {identity}@{}", reference.span.start),
                    );
                }
            }
            let prefix = expression
                .class
                .captured_type_parameters
                .iter()
                .map(parameter_reference)
                .collect::<Vec<_>>();
            let normalize = |value: &Type| {
                super::super::namespaces::rename_type_names(value, &rename, Some((name, &prefix)))
            };
            if let Some(definition) = self.types.get_mut(identity) {
                definition.value = normalize(&definition.value);
            }
            if let Some(value) = self.values.get_mut(identity) {
                *value = normalize(value);
            }
            if let Some(binding) = self.class_constructors.get_mut(identity) {
                for signature in &mut binding.signatures {
                    for parameter in &mut signature.parameters {
                        parameter.annotation = parameter.annotation.as_ref().map(normalize);
                    }
                    signature.return_type = normalize(&signature.return_type);
                }
            }
            if let Some(constructor) = self.values.get(identity).cloned() {
                for reference in &self.module.type_references {
                    if reference.value_query
                        && reference.name == *name
                        && self.expression_self_query(identity, name, reference.span.start)
                    {
                        let query = format!("typeof {name}@{}", reference.span.start);
                        if let Some(definition) = self.types.get_mut(&query) {
                            definition.value = constructor.clone();
                        }
                    }
                }
            }
        }
        self.bind_class_expression_aliases();
    }

    pub(in crate::checker::module::binding) fn check_nested_class_expressions(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) {
        let mut index = 0;
        while index < tokens.len() {
            let start = tokens[index].start;
            if let Some(function) = self.module.nested_functions.get(&start) {
                // Its own checker installs parameters and captured values first.
                index = tokens
                    .partition_point(|token| token.start < function.span.end)
                    .max(index + 1);
            } else if let Some(expression) = self.module.class_expression(start) {
                let end = tokens.partition_point(|token| token.start < expression.class.span.end);
                self.check_class_expression(&tokens[index..end], scope);
                index = end.max(index + 1);
            } else {
                index += 1;
            }
        }
    }

    pub(in crate::checker::module::binding) fn check_class_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
    ) -> bool {
        let tokens = strip_outer_parentheses(tokens);
        let Some(expression) = tokens
            .first()
            .and_then(|first| self.module.class_expression(first.start))
            .filter(|expression| {
                tokens
                    .last()
                    .is_some_and(|last| last.end == expression.class.span.end)
            })
        else {
            return false;
        };
        if !self
            .checked_class_expressions
            .insert(expression.class.span.start)
        {
            return true;
        }
        if !class_is_fully_structured(&expression.class) {
            self.diagnostics.push(self.class_shape_counterpart(
                &expression.class,
                Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    expression.class.span.clone(),
                    "a class expression member has no structured runtime representation",
                ),
            ));
            return true;
        }
        let saved_values = self.values.clone();
        self.values.extend(scope.clone());
        let captures = captured_parameter_substitutions(&expression.class);
        for value in self.values.values_mut() {
            *value = substitute_type(value, &captures);
        }
        let saved_identity_constructor =
            self.class_constructors.get(&expression.class.name).cloned();
        if let Some(binding) = self.class_constructors.get_mut(&expression.class.name) {
            for signature in &mut binding.signatures {
                for parameter in &mut signature.parameters {
                    parameter.annotation = parameter
                        .annotation
                        .as_ref()
                        .map(|value| substitute_type(value, &captures));
                }
                signature.return_type = substitute_type(&signature.return_type, &captures);
            }
        }
        let saved_type = expression.name.as_ref().and_then(|name| {
            let mut definition = self.types.get(&expression.class.name)?.clone();
            definition.parameters = class_definition_parameters(&expression.class)
                .into_iter()
                .skip(expression.class.captured_type_parameters.len())
                .collect();
            Some((name.clone(), self.types.insert(name.clone(), definition)))
        });
        let saved_constructor = expression.name.as_ref().and_then(|name| {
            let binding = self.class_constructors.get(&expression.class.name)?.clone();
            self.values.insert(
                name.clone(),
                self.values.get(&expression.class.name)?.clone(),
            );
            Some((
                name.clone(),
                self.class_constructors.insert(name.clone(), binding),
            ))
        });
        self.check_class_structure(&expression.class);
        if let Some((name, previous)) = saved_type {
            if let Some(previous) = previous {
                self.types.insert(name, previous);
            } else {
                self.types.remove(&name);
            }
        }
        if let Some((name, previous)) = saved_constructor {
            if let Some(previous) = previous {
                self.class_constructors.insert(name, previous);
            } else {
                self.class_constructors.remove(&name);
            }
        }
        self.values = saved_values;
        if let Some(binding) = saved_identity_constructor {
            self.class_constructors
                .insert(expression.class.name.clone(), binding);
        }
        true
    }
}
