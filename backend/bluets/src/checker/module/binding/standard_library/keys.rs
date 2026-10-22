// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Intrinsic key identity is static library metadata, not a runtime capability.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn bind_library_keys(
        &mut self,
        protected: &BTreeSet<String>,
        protected_values: &BTreeSet<String>,
    ) {
        let Some(target) = self.library_target else {
            return;
        };
        let modules = crate::standard_library::modules(target);
        let mut interfaces: BTreeMap<&str, Vec<&TypeField>> = BTreeMap::new();
        let mut values = BTreeMap::new();
        for module in modules {
            for declaration in &module.declarations {
                match declaration {
                    Declaration::Interface(interface) => interfaces
                        .entry(&interface.name)
                        .or_default()
                        .extend(&interface.fields),
                    Declaration::Variable(variable) => {
                        values.insert(variable.name.as_str(), variable.annotation.as_ref());
                    }
                    _ => {}
                }
            }
        }
        let mut intrinsic = BTreeMap::new();
        for (global, value) in values {
            let Some(Type::Named { name, .. }) = value else {
                continue;
            };
            for field in interfaces.get(name.as_str()).into_iter().flatten() {
                if matches!(field.value, Type::UniqueSymbol(_)) {
                    intrinsic.insert(format!("{global}.{}", field.name), field.value.clone());
                }
            }
        }
        let mut keys = Vec::new();
        for fields in interfaces.values() {
            for field in fields {
                if let Some(entity) = crate::symbol_keys::entity(&field.name) {
                    if let Some(value) = intrinsic.get(entity) {
                        keys.push((field.name.clone(), value.clone()));
                    }
                }
            }
        }
        for (key, value) in keys {
            self.types.insert(
                key,
                TypeDefinition {
                    kind: TypeDefinitionKind::Alias,
                    parameters: Vec::new(),
                    value,
                },
            );
        }
        for entity in intrinsic.keys() {
            let mut parts = entity.split('.');
            let Some(root) = parts.next() else {
                continue;
            };
            if !self.require_declared_global_calls && !protected_values.contains(root) {
                self.types.insert(
                    crate::symbol_keys::name(&self.module.id, entity),
                    TypeDefinition {
                        kind: TypeDefinitionKind::Alias,
                        parameters: Vec::new(),
                        value: intrinsic[entity].clone(),
                    },
                );
                continue;
            }
            let Some(mut value) = self.values.get(root).cloned() else {
                continue;
            };
            let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
            let mut found = true;
            for part in parts {
                match property_type(&value, part, &self.types, &mut HashSet::new(), &mut budget) {
                    PropertyType::Found { value: next, .. } => value = next,
                    _ => {
                        found = false;
                        break;
                    }
                }
            }
            if found && !budget.exhausted && matches!(value, Type::UniqueSymbol(_) | Type::Symbol) {
                self.types.insert(
                    crate::symbol_keys::name(&self.module.id, entity),
                    TypeDefinition {
                        kind: TypeDefinitionKind::Alias,
                        parameters: Vec::new(),
                        value,
                    },
                );
            }
        }
        // This native intrinsic depends on the existing strictness switch.
        if !protected.contains("BuiltinIteratorReturn") {
            let value = if self.checking.strict_builtin_iterator_return {
                Type::Undefined
            } else {
                Type::Any
            };
            let substitutions = BTreeMap::from([("BuiltinIteratorReturn".into(), value.clone())]);
            for (name, definition) in &mut self.types {
                if !protected.contains(name)
                    && definition.kind == TypeDefinitionKind::LibraryInterface
                {
                    definition.value = substitute_type(&definition.value, &substitutions);
                }
            }
            if let Some(definition) = self.types.get_mut("BuiltinIteratorReturn") {
                definition.value = value;
            }
        }
    }
}
