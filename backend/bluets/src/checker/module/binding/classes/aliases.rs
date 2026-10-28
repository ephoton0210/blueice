// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Constructor aliases retain captured arguments and original class capabilities.

use super::*;

impl ModuleChecker<'_> {
    pub(in crate::checker::module::binding) fn bind_class_expression_aliases(&mut self) {
        let mut scope = self.values.clone();
        for declaration in &self.module.declarations {
            let Declaration::Variable(variable) = declaration else {
                continue;
            };
            if variable.annotation.is_some() {
                continue;
            }
            let mut value = self.infer_variable_type(variable, &scope);
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let mut visited = HashSet::new();
            while matches!(value, Type::Named { .. }) {
                let Some(expanded) = instantiate_named(
                    &value,
                    &self.types,
                    &mut visited,
                    &mut budget,
                    "constructor alias",
                ) else {
                    break;
                };
                value = expanded;
            }
            if let Type::Named { name, .. } = &value {
                if let Some(query) = crate::parser::source_type_name(name).strip_prefix("typeof ") {
                    if let Some((name, module)) = query.rsplit_once('@') {
                        let name = name
                            .rsplit_once('@')
                            .filter(|(_, offset)| offset.parse::<usize>().is_ok())
                            .map_or(name, |(name, _)| name);
                        if let Some(class) = self
                            .exports
                            .classes
                            .get(module)
                            .and_then(|classes| classes.get(name))
                            .cloned()
                        {
                            let identity = format!("{name}@{module}");
                            if !self.types.contains_key(&identity) {
                                self.bind_imported_class(&identity, &class, &variable.span);
                            }
                            if let Some(constructor) = self.values.get(&identity) {
                                value = constructor.clone();
                            }
                        }
                    } else if let Some(constructor) = self.values.get(query) {
                        value = constructor.clone();
                    }
                }
            }
            if budget.exhausted {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::ResourceLimit,
                    variable.span.clone(),
                    "constructor alias exceeds the configured type-expansion limit",
                ));
                continue;
            }
            let Type::CallableRecord { signatures, .. } = value.object_type() else {
                continue;
            };
            let Some(identity) = signatures.iter().find_map(|signature| {
                if let (true, Type::Named { name, .. }) = (signature.construct, &signature.result) {
                    Some(name)
                } else {
                    None
                }
            }) else {
                continue;
            };
            let Some(mut binding) = self.class_constructors.get(identity).cloned() else {
                continue;
            };
            // A factory result has already substituted outer type arguments.
            // Its constructor still quantifies only its own class parameters.
            binding.signatures = signatures
                .iter()
                .filter(|signature| signature.construct)
                .map(|signature| FunctionSignature {
                    parameters: signature.parameters.clone(),
                    type_parameters: signature.type_parameters.clone(),
                    return_type: signature.result.clone(),
                })
                .collect();
            self.class_constructors
                .insert(variable.name.clone(), binding);
            scope.insert(variable.name.clone(), value.clone());
            self.values.insert(variable.name.clone(), value);
        }
    }
}
