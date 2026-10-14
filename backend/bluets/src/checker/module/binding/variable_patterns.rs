// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original flat variable pattern type projection; runtime evaluation stays
//! in the retained source and is never duplicated by these checker facts.

use super::*;

impl ModuleChecker<'_> {
    pub(super) fn variable_pattern_bindings(
        &mut self,
        variable: &crate::parser::VariableDeclaration,
        scope: &BTreeMap<String, Type>,
    ) -> BTreeMap<String, Type> {
        let Some(pattern) = &variable.pattern else {
            return BTreeMap::new();
        };
        let value = variable
            .annotation
            .clone()
            .unwrap_or_else(|| self.infer_variable_type(variable, scope));
        let mut bound = scope.clone();
        self.bind_pattern(&pattern.pattern, &value, &variable.span, &mut bound);
        if let BindingPattern::Object(bindings) = &pattern.pattern {
            if pattern.rest.is_none() {
                let contextual = Type::Record(
                    bindings
                        .iter()
                        .map(|binding| TypeField {
                            accessor_write_type: None,
                            method: false,
                            name: binding.key.clone(),
                            readonly: false,
                            optional: false,
                            value: Type::Any,
                            span: binding.span.clone(),
                        })
                        .collect(),
                );
                if self.check_fresh_properties(
                    &variable.initializer,
                    &contextual,
                    scope,
                    &variable.span,
                ) {
                    // This new binding boundary reports its fresh property at
                    // the actual key. Preserve older assignment boundaries'
                    // legacy spans while keeping these diagnostics in source order.
                    if let Some(diagnostic) = self.diagnostics.last_mut() {
                        if let Some(counterpart) = &diagnostic.typescript {
                            diagnostic.span = counterpart.span.clone();
                        }
                    }
                }
            }
        }
        if let Some(rest) = &pattern.rest {
            let mut resolved = value.clone();
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let mut visited = HashSet::new();
            while let Some(expanded) = instantiate_named(
                &resolved,
                &self.types,
                &mut visited,
                &mut budget,
                "variable rest pattern",
            ) {
                resolved = expanded;
            }
            if let Type::Readonly(inner) = resolved {
                resolved = *inner;
            }
            let rest_type = match (&pattern.pattern, resolved) {
                (_, Type::Any) => Type::Any,
                (
                    BindingPattern::Object(bindings),
                    Type::Record(fields) | Type::CallableRecord { fields, .. },
                ) => {
                    let excluded: BTreeSet<_> = bindings
                        .iter()
                        .map(|binding| binding.key.as_str())
                        .collect();
                    Type::Record(
                        fields
                            .into_iter()
                            .filter(|field| !excluded.contains(field.name.as_str()))
                            .map(|mut field| {
                                field.readonly = false;
                                field.accessor_write_type = None;
                                field
                            })
                            .collect(),
                    )
                }
                (BindingPattern::Array(_), Type::Array(element)) => Type::Array(element),
                (BindingPattern::Array(elements), Type::Tuple(items)) => {
                    Type::Tuple(items.into_iter().skip(elements.len()).collect())
                }
                _ => {
                    self.type_error(
                        &rest.span,
                        "this variable rest source type is not supported yet".to_string(),
                        DiagnosticCode::UnsupportedSyntax,
                    );
                    Type::Unknown
                }
            };
            bound.insert(rest.name.clone(), rest_type);
        }
        pattern
            .names()
            .into_iter()
            .filter_map(|(name, _)| {
                bound
                    .get(name)
                    .cloned()
                    .map(|value| (name.to_string(), value))
            })
            .collect()
    }
}
