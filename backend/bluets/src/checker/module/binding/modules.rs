// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Import/export binding and module-system validation.

use super::*;

pub(super) fn specialize_imported_class_type(
    value: &Type,
    source_name: &str,
    local_name: &str,
) -> Type {
    let substitution = BTreeMap::from([(
        source_name.to_string(),
        Type::Named {
            name: local_name.to_string(),
            arguments: Vec::new(),
        },
    )]);
    substitute_type(value, &substitution)
}

impl<'a> ModuleChecker<'a> {
    pub(in crate::checker::module) fn validate_default_exports(&mut self) {
        let tokens = crate::syntax::lex(&self.module.id, &self.module.source).unwrap_or_default();
        let defaults = self
            .module
            .declarations
            .iter()
            .flat_map(|declaration| match declaration {
                Declaration::DefaultExport(export) => vec![if export.expression {
                    export.span.clone()
                } else {
                    tokens
                        .iter()
                        .find(|token| {
                            token.start >= export.span.start
                                && token.end <= export.span.end
                                && token.text == export.name
                        })
                        .map(|token| token.span(&self.module.id))
                        .unwrap_or_else(|| export.span.clone())
                }],
                Declaration::Function(function) if function.default_export => {
                    vec![function.span.clone()]
                }
                Declaration::Class(class) if class.default_export => vec![class.span.clone()],
                Declaration::ValueExport(export) => export
                    .bindings
                    .iter()
                    .filter(|binding| binding.exported == "default")
                    .map(|binding| {
                        tokens
                            .iter()
                            .find(|token| {
                                token.start >= binding.span.start
                                    && token.end <= binding.span.end
                                    && token.is("default")
                            })
                            .map(|token| token.span(&self.module.id))
                            .unwrap_or_else(|| binding.span.clone())
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .collect::<Vec<_>>();
        if defaults.len() > 1 {
            for span in &defaults {
                let mut diagnostic = Diagnostic::error(
                    DiagnosticCode::DuplicateDeclaration,
                    span.clone(),
                    "a module can have only one default export",
                )
                .with_typescript(2528, Vec::new());
                diagnostic
                    .typescript
                    .as_mut()
                    .unwrap()
                    .related_information
                    .extend(defaults.iter().filter(|other| *other != span).map(|other| {
                        crate::TypeScriptRelatedInformation {
                            code: 2753,
                            message: "Another export default is here.".to_string(),
                            span: other.clone(),
                            position: None,
                        }
                    }));
                self.diagnostics.push(diagnostic);
            }
        }

        for declaration in &self.module.declarations {
            let Declaration::DefaultExport(export) = declaration else {
                continue;
            };
            let has_local_runtime_binding =
                self.module
                    .declarations
                    .iter()
                    .any(|candidate| match candidate {
                        Declaration::Variable(variable) => {
                            variable.name == export.name && !variable.declared
                        }
                        Declaration::Function(function) => {
                            function.name == export.name && !function.declared && !function.overload
                        }
                        Declaration::Class(class) => class.name == export.name,
                        Declaration::Enum(item) => item.name == export.name && !item.declared,
                        Declaration::Namespace(item) => {
                            item.name == export.name
                                && !item.declared
                                && namespaces::body_has_values(&item.body)
                        }
                        Declaration::Import(import) => {
                            !import.type_only
                                && import.bindings.iter().any(|binding| {
                                    binding.local == export.name && !binding.type_only
                                })
                        }
                        _ => false,
                    });
            if !has_local_runtime_binding {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnknownName,
                    crate::syntax::lex(&self.module.id, &self.module.source)
                        .unwrap_or_default()
                        .iter()
                        .find(|token| {
                            token.start >= export.span.start
                                && token.end <= export.span.end
                                && token.text == export.name
                        })
                        .map(|token| token.span(&self.module.id))
                        .unwrap_or_else(|| export.span.clone()),
                    format!(
                        "default export `{}` must name a local runtime declaration",
                        export.name
                    ),
                ));
            }
        }
    }

    pub(in crate::checker::module) fn validate_value_exports(&mut self) {
        let mut exported_names = BTreeSet::new();
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Variable(variable) if variable.exported && !variable.declared => {
                    exported_names.insert(variable.name.clone());
                }
                Declaration::Function(function)
                    if function.exported && !function.default_export && !function.overload =>
                {
                    exported_names.insert(function.name.clone());
                }
                Declaration::Class(class) if class.exported => {
                    exported_names.insert(class.name.clone());
                }
                _ => {}
            }
        }

        for declaration in &self.module.declarations {
            let Declaration::ValueExport(export) = declaration else {
                continue;
            };
            if export.specifier.is_some() {
                self.validate_reexport(export);
                continue;
            }
            for binding in &export.bindings {
                let has_local_runtime_binding =
                    self.module
                        .declarations
                        .iter()
                        .any(|candidate| match candidate {
                            Declaration::Variable(variable) => {
                                variable.name == binding.local && !variable.declared
                            }
                            Declaration::Function(function) => {
                                function.name == binding.local
                                    && !function.declared
                                    && !function.overload
                            }
                            Declaration::Class(class) => class.name == binding.local,
                            Declaration::Enum(declaration) => {
                                declaration.name == binding.local && !declaration.declared
                            }
                            Declaration::Namespace(namespace) => {
                                namespace.name == binding.local
                                    && (is_declaration_module(&self.module.id)
                                        || (!namespace.declared
                                            && namespaces::body_has_values(&namespace.body)))
                            }
                            _ => false,
                        });
                if !has_local_runtime_binding {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnknownName,
                        binding.span.clone(),
                        format!(
                            "exported value `{}` must name a local runtime declaration",
                            binding.local
                        ),
                    ));
                }
                if !exported_names.insert(binding.exported.clone()) {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::DuplicateDeclaration,
                        binding.span.clone(),
                        format!("duplicate exported value `{}`", binding.exported),
                    ));
                }
            }
        }
    }

    /// `import x = require()` and `export =` belong to CommonJS, and an `export
    /// =` module has no other exports.
    pub(super) fn validate_module_system(&mut self) {
        use crate::compiler::ModuleKind;
        let mut assignment: Option<&crate::parser::ValueExportDeclaration> = None;
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Import(import)
                    if import.equals_require && self.module_kind == ModuleKind::Esm =>
                {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        import.span.clone(),
                        "`import x = require()` cannot be used when the module system is ECMAScript; use `--module commonjs`",
                    ));
                }
                Declaration::ValueExport(export) if export.export_assignment => {
                    if self.module_kind == ModuleKind::Esm {
                        self.diagnostics.push(Diagnostic::error(
                            DiagnosticCode::UnsupportedSyntax,
                            export.span.clone(),
                            "`export =` cannot be used when the module system is ECMAScript; use `--module commonjs`",
                        ));
                    }
                    assignment = Some(export);
                }
                _ => {}
            }
        }
        let Some(assignment) = assignment else {
            return;
        };
        let has_other_exports =
            self.module
                .declarations
                .iter()
                .any(|declaration| match declaration {
                    Declaration::ValueExport(export) => !export.export_assignment,
                    Declaration::DefaultExport(_) | Declaration::TypeExport(_) => true,
                    Declaration::Variable(item) => item.exported,
                    Declaration::Function(item) => item.exported,
                    Declaration::Class(item) => item.exported,
                    Declaration::Enum(item) => item.exported,
                    Declaration::Interface(item) => item.exported,
                    Declaration::TypeAlias(item) => item.exported,
                    Declaration::Namespace(item) => item.exported,
                    _ => false,
                });
        if has_other_exports {
            self.diagnostics.push(Diagnostic::error(
                DiagnosticCode::TypeMismatch,
                assignment.span.clone(),
                "an export assignment cannot be used in a module with other exported elements",
            ));
        }
    }

    pub(in crate::checker::module) fn bind_import(
        &mut self,
        import: &crate::parser::ImportDeclaration,
    ) {
        let Some(resolved) = self
            .project
            .resolutions
            .get(&(self.module.id.clone(), import.specifier.clone()))
        else {
            return;
        };
        let exported = self.exports.types.get(resolved);
        let exported_classes = self.exports.classes.get(resolved);
        let mut merged_namespaces: Vec<(String, NamespaceExport, bool)> = Vec::new();
        for binding in &import.bindings {
            // A module that is `export =` has no `default`: its default import is
            // its whole export, which TypeScript allows only with interop.
            let remapped;
            let binding = if binding.imported == "default"
                && self
                    .exports
                    .exported_names
                    .get(resolved)
                    .is_some_and(|(names, _)| {
                        names.contains("export=") && !names.contains("default")
                    }) {
                if !self.es_module_interop {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::TypeMismatch,
                        import.span.clone(),
                        format!(
                            "module `{}` uses `export =` and can only be default-imported with `esModuleInterop`",
                            import.specifier
                        ),
                    ));
                    continue;
                }
                remapped = crate::parser::ImportBinding {
                    imported: "export=".to_string(),
                    ..binding.clone()
                };
                &remapped
            } else {
                binding
            };
            if let Some((source, _)) = crate::checker::reexports::origins(self.project)
                .get(resolved)
                .and_then(|names| names.get(&binding.imported))
                .filter(|origins| origins.len() == 1)
                .and_then(|origins| origins.first())
                .filter(|(_, name)| name == "*")
                .cloned()
            {
                self.bind_module_namespace(&binding.local, &source, &import.span, false);
                continue;
            }
            if let Some(namespace) = self
                .namespace_exports
                .get(resolved)
                .and_then(|namespaces| namespaces.get(&binding.imported))
            {
                // A namespace merged into a class, enum or function of the same
                // name is bound beside it, which is bound as usual.
                let merged = exported_classes
                    .is_some_and(|classes| classes.contains_key(&binding.imported))
                    || self
                        .exports
                        .enums
                        .get(resolved)
                        .is_some_and(|enums| enums.contains_key(&binding.imported))
                    || self.project.modules.get(resolved).is_some_and(|module| {
                        module.declarations.iter().any(|declaration| {
                            matches!(declaration, Declaration::Function(function)
                                if function.name == binding.imported && !function.declared)
                        })
                    });
                if merged {
                    // Bound after the class, enum or function it merges into.
                    merged_namespaces.push((
                        binding.local.clone(),
                        namespace.clone(),
                        !binding.type_only,
                    ));
                } else {
                    self.bind_imported_namespace(
                        &binding.local,
                        namespace,
                        &import.span,
                        !binding.type_only,
                        false,
                    );
                    continue;
                }
            }
            if binding.imported == "*" {
                // `import * as ns` makes the module's types nameable as `ns.T`
                // (a value import also keeps `ns` itself as a value below).
                if let Some(source_types) = exported {
                    if binding.type_only
                        && exported_classes.is_some_and(|classes| !classes.is_empty())
                    {
                        self.type_only_classes.insert(binding.local.clone());
                    }
                    for (name, definition) in source_types {
                        let local = format!("{}.{}", binding.local, name);
                        let mut definition = definition.clone();
                        if let Some(class) = exported_classes.and_then(|classes| classes.get(name))
                        {
                            definition.value = specialize_imported_class_type(
                                &class.instance_type,
                                &class.source_name,
                                &local,
                            );
                        }
                        self.insert_type(
                            &local,
                            definition,
                            import.span.clone(),
                            SymbolKind::Import,
                            false,
                        );
                    }
                }
                // The module's namespaces are members of the imported object:
                // `ns.JSX.Element`, `ns.Ui.f`.
                if let Some(namespaces) = self.namespace_exports.get(resolved) {
                    for (name, namespace) in namespaces {
                        let local = format!("{}.{name}", binding.local);
                        let with_value = !binding.type_only && namespace.has_values;
                        self.bind_imported_namespace(
                            &local,
                            namespace,
                            &import.span,
                            with_value,
                            false,
                        );
                    }
                }
                if binding.type_only {
                    continue;
                }
            }
            if binding.type_only {
                if let Some(enum_) = self
                    .exports
                    .enums
                    .get(resolved)
                    .and_then(|enums| enums.get(&binding.imported))
                    .cloned()
                {
                    self.bind_imported_enum(&binding.local, &enum_, &import.span, false);
                    continue;
                }
                let Some(source_type) = exported.and_then(|types| types.get(&binding.imported))
                else {
                    self.diagnostics.push(Diagnostic::error(
                        DiagnosticCode::UnknownType,
                        import.span.clone(),
                        format!(
                            "module `{}` has no exported type `{}`",
                            import.specifier, binding.imported
                        ),
                    ));
                    continue;
                };
                let mut source_type = source_type.clone();
                if let Some(class) =
                    exported_classes.and_then(|classes| classes.get(&binding.imported))
                {
                    self.type_only_classes.insert(binding.local.clone());
                    source_type.value = specialize_imported_class_type(
                        &class.instance_type,
                        &class.source_name,
                        &binding.local,
                    );
                }
                self.insert_type(
                    &binding.local,
                    source_type,
                    import.span.clone(),
                    SymbolKind::Import,
                    false,
                );
            } else {
                // An export assignment's public object properties are named exports.
                let requires_namespace = import.equals_require
                    && !self
                        .exports
                        .exported_names
                        .get(resolved)
                        .is_some_and(|(names, _)| names.contains("export="));
                if binding.imported != "*"
                    && binding.imported != "export="
                    && binding.imported != "default"
                    && self.bind_export_assignment_member(
                        &binding.local,
                        &binding.imported,
                        resolved,
                        &import.span,
                    )
                {
                    continue;
                }
                // A value import must name something the module exports.
                if binding.imported != "*"
                    && !requires_namespace
                    && !is_declaration_module(resolved)
                    && self
                        .exports
                        .exported_names
                        .get(resolved)
                        .is_some_and(|(names, open)| !open && !names.contains(&binding.imported))
                {
                    let names = &self.exports.exported_names.get(resolved).unwrap().0;
                    let suggestion = crate::diagnostic::spelling::suggestion(
                        &binding.imported,
                        names.iter().map(String::as_str),
                    );
                    let (typescript_code, arguments) = if binding.imported == "default" {
                        if names.contains(&binding.local) {
                            (2613, vec![import.specifier.clone(), binding.local.clone()])
                        } else {
                            (1192, vec![import.specifier.clone()])
                        }
                    } else if let Some(suggestion) = suggestion {
                        (
                            2724,
                            vec![
                                import.specifier.clone(),
                                binding.imported.clone(),
                                suggestion.into(),
                            ],
                        )
                    } else {
                        (
                            2305,
                            vec![import.specifier.clone(), binding.imported.clone()],
                        )
                    };
                    let diagnostic = Diagnostic::error(
                        DiagnosticCode::UnknownName,
                        import.span.clone(),
                        format!(
                            "module `{}` has no exported member `{}`",
                            import.specifier, binding.imported
                        ),
                    )
                    .with_typescript(typescript_code, arguments);
                    let diagnostic = if typescript_code == 1192 {
                        self.refine_missing_default(import, binding, resolved, diagnostic)
                    } else {
                        diagnostic
                    };
                    self.diagnostics.push(diagnostic);
                    continue;
                }
                if let Some(class) = exported_classes
                    .and_then(|classes| classes.get(&binding.imported))
                    .cloned()
                {
                    if class.value_exported {
                        self.bind_imported_class(&binding.local, &class, &import.span);
                    } else {
                        self.type_only_classes.insert(binding.local.clone());
                        self.insert_type(
                            &binding.local,
                            TypeDefinition {
                                kind: TypeDefinitionKind::Class,
                                parameters: Vec::new(),
                                value: specialize_imported_class_type(
                                    &class.instance_type,
                                    &class.source_name,
                                    &binding.local,
                                ),
                            },
                            import.span.clone(),
                            SymbolKind::Import,
                            false,
                        );
                    }
                    continue;
                }
                if let Some(enum_) = self
                    .exports
                    .enums
                    .get(resolved)
                    .and_then(|enums| enums.get(&binding.imported))
                    .cloned()
                {
                    self.bind_imported_enum(&binding.local, &enum_, &import.span, true);
                    continue;
                }
                if binding.imported == "*"
                    || (import.equals_require
                        && !self
                            .exports
                            .exported_names
                            .get(resolved)
                            .is_some_and(|(names, _)| names.contains("export=")))
                {
                    self.bind_module_namespace(
                        &binding.local,
                        resolved,
                        &import.span,
                        !import.equals_require,
                    );
                } else if let Some(value) = self
                    .exports
                    .values
                    .get(resolved)
                    .and_then(|values| values.get(&binding.imported))
                    .cloned()
                {
                    self.bind_imported_value(&binding.local, &value, &import.span);
                } else if let Some(definition) =
                    exported.and_then(|types| types.get(&binding.imported))
                {
                    self.insert_type(
                        &binding.local,
                        definition.clone(),
                        import.span.clone(),
                        SymbolKind::Import,
                        false,
                    );
                } else {
                    self.insert_value(
                        &binding.local,
                        Type::Unknown,
                        import.span.clone(),
                        SymbolKind::Import,
                        false,
                    );
                }
            }
        }
        for (local, namespace, with_value) in merged_namespaces {
            self.bind_imported_namespace(&local, &namespace, &import.span, with_value, true);
        }
    }

    pub(in crate::checker::module) fn bind_type_export(
        &mut self,
        export: &crate::parser::TypeExportDeclaration,
    ) {
        let source_types = export.specifier.as_ref().and_then(|specifier| {
            self.project
                .resolutions
                .get(&(self.module.id.clone(), specifier.clone()))
                .and_then(|resolved| self.exports.types.get(resolved))
        });
        for binding in &export.bindings {
            if binding == "*" {
                continue;
            }
            let local = binding.split(" as ").next().unwrap_or(binding);
            let exists = source_types.is_some_and(|types| types.contains_key(local))
                || export.specifier.is_none() && self.types.contains_key(local);
            if !exists {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnknownType,
                    export.span.clone(),
                    format!("cannot re-export unknown type `{local}`"),
                ));
            }
        }
    }
}
