// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exported surfaces carry their closed static type dependencies through barrels.
use super::*;

impl ModuleChecker<'_> {
    pub(super) fn namespace_export_of(&self, source: &str) -> NamespaceExport {
        let nested = format!("{source}.");
        let object = format!("typeof {source}");
        let object_nested = format!("typeof {source}.");
        let belongs = |key: &str| {
            key == source
                || key.starts_with(&nested)
                || key == object
                || key.starts_with(&object_nested)
        };
        let mut export = NamespaceExport {
            source_name: source.to_string(),
            values: self
                .values
                .iter()
                .filter(|(key, _)| belongs(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            functions: self
                .functions
                .iter()
                .filter(|(key, _)| belongs(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            class_constructors: self
                .class_constructors
                .iter()
                .filter(|(key, _)| belongs(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            types: self
                .types
                .iter()
                .filter(|(key, _)| belongs(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            members: self
                .namespaces
                .iter()
                .filter(|(path, _)| path.as_str() == source || path.starts_with(&nested))
                .map(|(path, members)| (path.clone(), members.clone()))
                .collect(),
            has_values: self.module.declarations.iter().any(|declaration| {
                matches!(declaration, Declaration::Namespace(namespace)
                    if namespace.name == source
                        && !namespace.declared
                        && body_has_values(&namespace.body))
            }),
        };
        // The module's own types the entries mention travel with them, under
        // keys that say which module they belong to.
        let mut local_roots: BTreeSet<String> = self
            .module
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Interface(item) => Some(item.name.clone()),
                Declaration::TypeAlias(item) => Some(item.name.clone()),
                Declaration::Class(item) => Some(item.name.clone()),
                Declaration::Enum(item) => Some(item.name.clone()),
                Declaration::Namespace(item) => Some(item.name.clone()),
                _ => None,
            })
            .collect();
        local_roots.extend(
            self.module
                .class_expressions()
                .map(|expression| expression.class.name.clone()),
        );
        local_roots.extend(
            self.module
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    Declaration::Import(import) => Some(import),
                    _ => None,
                })
                .flat_map(|import| import.bindings.iter())
                .filter(|binding| self.types.contains_key(&binding.local))
                .map(|binding| binding.local.clone()),
        );
        let mut dependencies: BTreeSet<String> = BTreeSet::new();
        let mut query_names = BTreeMap::new();
        let mut pending: Vec<Type> = Vec::new();
        let scan = |export: &NamespaceExport, pending: &mut Vec<Type>| {
            pending.extend(export.values.values().cloned());
            for signatures in export.functions.values() {
                for signature in signatures {
                    pending.push(Type::GenericFunction {
                        type_parameters: signature.type_parameters.clone(),
                        parameters: signature.parameters.clone(),
                        result: Box::new(signature.return_type.clone()),
                        span: SourceSpan::new(&self.module.id, 0, 0),
                    });
                }
            }
            for binding in export.class_constructors.values() {
                for signature in &binding.signatures {
                    pending.push(Type::GenericFunction {
                        type_parameters: signature.type_parameters.clone(),
                        parameters: signature.parameters.clone(),
                        result: Box::new(signature.return_type.clone()),
                        span: SourceSpan::new(&self.module.id, 0, 0),
                    });
                }
            }
            pending.extend(
                export
                    .types
                    .values()
                    .map(|definition| definition.value.clone()),
            );
        };
        scan(&export, &mut pending);
        while let Some(value) = pending.pop() {
            let mut names = BTreeSet::new();
            named_types(&value, &mut names);
            for name in names {
                let spelling = crate::parser::source_type_name(&name);
                let bare = spelling.strip_prefix("typeof ").unwrap_or(spelling);
                let root = bare.split('.').next().unwrap_or(bare);
                if !local_roots.contains(root) || belongs(bare) {
                    continue;
                }
                if name != spelling {
                    query_names.insert(name.clone(), format!("{spelling}@{}", self.module.id));
                }
                if dependencies.insert(root.to_string()) {
                    let root_nested = format!("{root}.");
                    let root_object = format!("typeof {root}");
                    for (key, definition) in &self.types {
                        if key == root
                            || key.starts_with(&root_nested)
                            || *key == root_object
                            || key.starts_with(&format!("typeof {root}."))
                        {
                            pending.push(definition.value.clone());
                        }
                    }
                }
            }
        }
        if dependencies.is_empty() {
            return export;
        }
        let mut rename: BTreeMap<String, String> = query_names;
        for root in &dependencies {
            let root_nested = format!("{root}.");
            let root_object = format!("typeof {root}");
            let root_object_nested = format!("typeof {root}.");
            for key in self.types.keys() {
                if key == root
                    || key.starts_with(&root_nested)
                    || *key == root_object
                    || key.starts_with(&root_object_nested)
                {
                    rename.insert(key.clone(), format!("{key}@{}", self.module.id));
                }
            }
        }
        let qualify = Qualifier {
            rename: &rename,
            argument_prefix: None,
        };
        export.values = export
            .values
            .iter()
            .map(|(key, value)| (key.clone(), qualify.ty(value)))
            .collect();
        export.functions = export
            .functions
            .iter()
            .map(|(key, value)| (key.clone(), qualify.signatures(value)))
            .collect();
        export.class_constructors = export
            .class_constructors
            .iter()
            .map(|(key, binding)| {
                (
                    key.clone(),
                    ClassConstructorBinding {
                        signatures: qualify.signatures(&binding.signatures),
                        ..binding.clone()
                    },
                )
            })
            .collect();
        export.types = export
            .types
            .iter()
            .map(|(key, definition)| (key.clone(), qualify.definition(definition)))
            .collect();
        for (old, new) in &rename {
            if let Some(definition) = self.types.get(old) {
                export
                    .types
                    .insert(new.clone(), qualify.definition(definition));
                if definition.kind == TypeDefinitionKind::Class {
                    if let Some(binding) = self.class_constructors.get(old) {
                        export.class_constructors.insert(
                            new.clone(),
                            ClassConstructorBinding {
                                signatures: qualify.signatures(&binding.signatures),
                                ..binding.clone()
                            },
                        );
                    }
                    if let Some(value) = self.values.get(old) {
                        export.values.insert(new.clone(), qualify.ty(value));
                    }
                }
            }
        }
        export
    }
}
