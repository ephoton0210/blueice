// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration binding and structural validation for one module.

use super::*;

impl<'a> ModuleChecker<'a> {
    pub(crate) fn new(
        project: &'a Project,
        module: &'a Module,
        exported_types: &'a BTreeMap<String, BTreeMap<String, TypeDefinition>>,
        ambient: Option<&'a AmbientDeclarations>,
        enforce_types: bool,
        require_declared_global_calls: bool,
        max_type_expansions: usize,
    ) -> Self {
        Self {
            project,
            module,
            exported_types,
            ambient,
            enforce_types,
            require_declared_global_calls,
            diagnostics: Vec::new(),
            symbols: Vec::new(),
            types: BTreeMap::new(),
            values: BTreeMap::new(),
            functions: BTreeMap::new(),
            function_implementations: BTreeSet::new(),
            type_parameters: BTreeSet::new(),
            max_type_expansions,
        }
    }

    pub(crate) fn bind(&mut self) {
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Import(import) => self.bind_import(import),
                Declaration::TypeExport(export) => self.bind_type_export(export),
                Declaration::DefaultExport(_) => {}
                Declaration::ValueExport(_) => {}
                Declaration::TypeAlias(alias) => {
                    self.insert_type(
                        &alias.name,
                        TypeDefinition {
                            kind: TypeDefinitionKind::Alias,
                            parameters: alias.type_parameters.clone(),
                            value: alias.value.clone(),
                        },
                        alias.span.clone(),
                        SymbolKind::TypeAlias,
                        alias.exported,
                    );
                }
                Declaration::Interface(interface) => {
                    self.insert_type(
                        &interface.name,
                        TypeDefinition {
                            kind: TypeDefinitionKind::Interface,
                            parameters: interface.type_parameters.clone(),
                            value: interface_value(interface),
                        },
                        interface.span.clone(),
                        SymbolKind::Interface,
                        interface.exported,
                    );
                }
                Declaration::Variable(variable) => {
                    let value_type = variable.annotation.clone().unwrap_or(Type::Unknown);
                    self.insert_value(
                        &variable.name,
                        value_type,
                        variable.span.clone(),
                        SymbolKind::Variable,
                        variable.exported,
                    );
                }
                Declaration::Function(function) => {
                    let value_type = function.return_type.clone().unwrap_or(Type::Unknown);
                    let signature = FunctionSignature {
                        parameters: function.parameters.clone(),
                        type_parameters: function.type_parameters.clone(),
                        return_type: function.return_type.clone().unwrap_or(Type::Unknown),
                    };
                    if !self.values.contains_key(&function.name) {
                        self.insert_value(
                            &function.name,
                            value_type,
                            function.span.clone(),
                            SymbolKind::Function,
                            function.exported,
                        );
                    } else if !self.functions.contains_key(&function.name) {
                        self.duplicate(&function.name, function.span.clone());
                        continue;
                    }
                    if function.overload || function.declared {
                        if self.function_implementations.contains(&function.name) {
                            self.type_error(
                                &function.span,
                                format!(
                                    "overload signature for {} must precede its implementation",
                                    function.name
                                ),
                                DiagnosticCode::TypeMismatch,
                            );
                        } else {
                            self.functions
                                .entry(function.name.clone())
                                .or_default()
                                .push(signature);
                        }
                    } else if !self.function_implementations.insert(function.name.clone()) {
                        self.duplicate(&function.name, function.span.clone());
                    } else if self.functions.get(&function.name).is_none_or(Vec::is_empty) {
                        self.functions
                            .entry(function.name.clone())
                            .or_default()
                            .push(signature);
                    }
                }
                Declaration::Raw(_) => {}
            }
        }
        self.bind_ambient_declarations();
        self.validate_function_overloads();
        self.validate_default_exports();
        self.validate_value_exports();
    }

    fn bind_ambient_declarations(&mut self) {
        let Some(ambient) = self.ambient else {
            return;
        };
        let local_types = self.types.keys().cloned().collect::<BTreeSet<_>>();
        let local_values = self.values.keys().cloned().collect::<BTreeSet<_>>();
        let local_functions = self.functions.keys().cloned().collect::<BTreeSet<_>>();
        for (name, definition) in &ambient.types {
            if !local_types.contains(name) {
                self.types.insert(name.clone(), definition.clone());
            }
        }
        for (name, value) in &ambient.values {
            if !local_values.contains(name) {
                self.values.insert(name.clone(), value.clone());
            }
        }
        for (name, signatures) in &ambient.functions {
            if !local_values.contains(name) && !local_functions.contains(name) {
                self.functions.insert(name.clone(), signatures.clone());
            }
        }
    }

    pub(super) fn validate_default_exports(&mut self) {
        let defaults = self
            .module
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::DefaultExport(export) => Some(export.span.clone()),
                Declaration::Function(function) if function.default_export => {
                    Some(function.span.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if defaults.len() > 1 {
            for span in defaults.iter().skip(1) {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::DuplicateDeclaration,
                    span.clone(),
                    "a module can have only one default export",
                ));
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
                        _ => false,
                    });
            if !has_local_runtime_binding {
                self.diagnostics.push(Diagnostic::error(
                    DiagnosticCode::UnknownName,
                    export.span.clone(),
                    format!(
                        "default export `{}` must name a local runtime declaration",
                        export.name
                    ),
                ));
            }
        }
    }

    pub(super) fn validate_value_exports(&mut self) {
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
                _ => {}
            }
        }

        for declaration in &self.module.declarations {
            let Declaration::ValueExport(export) = declaration else {
                continue;
            };
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

    pub(super) fn validate_function_overloads(&mut self) {
        let overloads = self
            .module
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Function(function) if function.overload => Some(function),
                _ => None,
            })
            .collect::<Vec<_>>();
        for overload in overloads {
            let implementations = self
                .module
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    Declaration::Function(function)
                        if function.name == overload.name
                            && !function.overload
                            && !function.declared =>
                    {
                        Some(function)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let Some(implementation) = implementations.first() else {
                if !is_declaration_module(&self.module.id) {
                    self.type_error(
                        &overload.span,
                        format!(
                            "overload signature for {} requires an implementation",
                            overload.name
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
                continue;
            };
            match overload_is_compatible_with_implementation(
                overload,
                implementation,
                &self.types,
                self.max_type_expansions,
            ) {
                Ok(true) => {}
                Ok(false) => self.type_error(
                    &overload.span,
                    format!(
                        "overload signature for {} is incompatible with its implementation",
                        overload.name
                    ),
                    DiagnosticCode::TypeMismatch,
                ),
                Err(()) => self.type_error(
                    &overload.span,
                    format!(
                        "overload compatibility exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                ),
            }
        }
    }

    pub(super) fn bind_import(&mut self, import: &crate::parser::ImportDeclaration) {
        let Some(resolved) = self
            .project
            .resolutions
            .get(&(self.module.id.clone(), import.specifier.clone()))
        else {
            return;
        };
        let exported = self.exported_types.get(resolved);
        for binding in &import.bindings {
            if binding.type_only {
                if binding.imported == "*" {
                    if let Some(source_types) = exported {
                        for (name, definition) in source_types {
                            self.insert_type(
                                &format!("{}.{}", binding.local, name),
                                definition.clone(),
                                import.span.clone(),
                                SymbolKind::Import,
                                false,
                            );
                        }
                    }
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
                self.insert_type(
                    &binding.local,
                    source_type.clone(),
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

    pub(super) fn bind_type_export(&mut self, export: &crate::parser::TypeExportDeclaration) {
        let source_types = export.specifier.as_ref().and_then(|specifier| {
            self.project
                .resolutions
                .get(&(self.module.id.clone(), specifier.clone()))
                .and_then(|resolved| self.exported_types.get(resolved))
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

    pub(super) fn insert_type(
        &mut self,
        name: &str,
        definition: TypeDefinition,
        span: SourceSpan,
        kind: SymbolKind,
        exported: bool,
    ) {
        if self
            .types
            .insert(name.to_string(), definition.clone())
            .is_some()
        {
            self.duplicate(name, span);
            return;
        }
        self.symbols.push(Symbol {
            name: name.to_string(),
            kind,
            module: self.module.id.clone(),
            span,
            exported,
            value_type: Some(definition.value),
        });
    }

    pub(super) fn insert_value(
        &mut self,
        name: &str,
        value_type: Type,
        span: SourceSpan,
        kind: SymbolKind,
        exported: bool,
    ) {
        if self
            .values
            .insert(name.to_string(), value_type.clone())
            .is_some()
        {
            self.duplicate(name, span);
            return;
        }
        self.symbols.push(Symbol {
            name: name.to_string(),
            kind,
            module: self.module.id.clone(),
            span,
            exported,
            value_type: Some(value_type),
        });
    }

    pub(super) fn duplicate(&mut self, name: &str, span: SourceSpan) {
        self.diagnostics.push(Diagnostic::error(
            DiagnosticCode::DuplicateDeclaration,
            span,
            format!("duplicate declaration of `{name}`"),
        ));
    }

    pub(crate) fn check_types(&mut self) {
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::TypeAlias(alias) => {
                    self.check_type_with_parameters(
                        &alias.value,
                        &alias.span,
                        &alias.type_parameters,
                    );
                }
                Declaration::Interface(interface) => {
                    for parent in &interface.heritage {
                        self.check_interface_heritage(
                            parent,
                            &interface.span,
                            &interface.type_parameters,
                        );
                        self.check_inherited_field_compatibility(
                            parent,
                            &interface.fields,
                            &interface.span,
                        );
                    }
                    for field in &interface.fields {
                        self.check_type_with_parameters(
                            &field.value,
                            &field.span,
                            &interface.type_parameters,
                        );
                    }
                }
                Declaration::Variable(variable) => self.check_variable(variable),
                Declaration::Function(function) => self.check_function(function),
                Declaration::Import(_)
                | Declaration::TypeExport(_)
                | Declaration::DefaultExport(_)
                | Declaration::ValueExport(_) => {}
                Declaration::Raw(raw) => {
                    let scope = self.values.clone();
                    self.check_direct_runtime_expression(&raw.tokens, &scope, &raw.span);
                }
            }
        }
    }

    pub(super) fn check_type_with_parameters(
        &mut self,
        value: &Type,
        span: &SourceSpan,
        parameters: &[TypeParameter],
    ) {
        let previous_parameters = self.type_parameters.clone();
        self.check_type_parameters(parameters);
        self.check_type(value, span);
        self.type_parameters = previous_parameters;
    }

    pub(super) fn check_interface_heritage(
        &mut self,
        parent: &Type,
        span: &SourceSpan,
        parameters: &[TypeParameter],
    ) {
        let previous_parameters = self.type_parameters.clone();
        self.check_type_parameters(parameters);
        self.check_type(parent, span);
        if let Type::Named { name, .. } = parent {
            match self.types.get(name) {
                Some(TypeDefinition {
                    kind: TypeDefinitionKind::Interface,
                    ..
                }) => {}
                Some(_) => self.type_error(
                    span,
                    format!("interface heritage {name} must name an interface declaration"),
                    DiagnosticCode::UnsupportedSyntax,
                ),
                None if self.type_parameters.contains(name) => self.type_error(
                    span,
                    format!("interface heritage {name} must name an interface declaration"),
                    DiagnosticCode::UnsupportedSyntax,
                ),
                None => {}
            }
        }
        self.type_parameters = previous_parameters;
    }

    pub(super) fn check_inherited_field_compatibility(
        &mut self,
        parent: &Type,
        fields: &[TypeField],
        span: &SourceSpan,
    ) {
        for field in fields {
            let inherited = {
                let mut visited = HashSet::new();
                let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                property_type(parent, &field.name, &self.types, &mut visited, &mut budget)
            };
            let PropertyType::Found(inherited) = inherited else {
                continue;
            };
            let declared = if field.optional {
                Type::Union(vec![field.value.clone(), Type::Undefined])
            } else {
                field.value.clone()
            };
            if !self.is_assignable_bounded(&declared, &inherited, span) {
                self.type_error(
                    span,
                    format!(
                        "property `{}` is not compatible with the inherited type `{}`",
                        field.name,
                        type_label(&inherited)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    pub(super) fn check_variable(&mut self, variable: &crate::parser::VariableDeclaration) {
        let scope = self.values.clone();
        self.check_variable_in_scope(variable, &scope);
    }

    pub(super) fn check_variable_in_scope(
        &mut self,
        variable: &crate::parser::VariableDeclaration,
        scope: &BTreeMap<String, Type>,
    ) {
        let Some(annotation) = &variable.annotation else {
            return;
        };
        self.check_type(annotation, &variable.span);
        if variable.initializer.is_empty() || variable.declared {
            return;
        }
        self.check_function_call(&variable.initializer, scope, &variable.span);
        self.check_direct_property_access(&variable.initializer, scope, &variable.span);
        self.check_arithmetic_operators(&variable.initializer, scope, &variable.span);
        let inferred = if matches!(annotation, Type::Tuple(_)) {
            infer_contextual_tuple_literal(&variable.initializer, scope)
                .unwrap_or_else(|| self.infer_expression(&variable.initializer, scope))
        } else {
            self.infer_expression(&variable.initializer, scope)
        };
        if !self.is_assignable_bounded(&inferred, annotation, &variable.span) {
            self.type_error(
                &variable.span,
                format!(
                    "initializer has type `{}`, which is not assignable to `{}`",
                    type_label(&inferred),
                    type_label(annotation)
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }

    pub(super) fn check_function(&mut self, function: &FunctionDeclaration) {
        let mut scope = self.values.clone();
        let previous_parameters = self.type_parameters.clone();
        self.check_type_parameters(&function.type_parameters);
        for (index, parameter) in function.parameters.iter().enumerate() {
            if parameter.rest && index + 1 != function.parameters.len() {
                self.type_error(
                    &parameter.span,
                    "a rest parameter must be last".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if parameter.rest && parameter.optional {
                self.type_error(
                    &parameter.span,
                    "a rest parameter cannot be optional or have a default initializer".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if parameter.rest
                && parameter
                    .annotation
                    .as_ref()
                    .is_some_and(|annotation| !matches!(annotation, Type::Array(_)))
            {
                self.type_error(
                    &parameter.span,
                    "the bounded rest-parameter rule requires an array annotation".to_string(),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if let Some(default) = &parameter.default {
                self.check_function_call(default, &scope, &parameter.span);
                self.check_direct_property_access(default, &scope, &parameter.span);
                self.check_arithmetic_operators(default, &scope, &parameter.span);
                let actual = self.infer_expression(default, &scope);
                let expected = parameter
                    .annotation
                    .as_ref()
                    .cloned()
                    .unwrap_or(Type::Unknown);
                if !self.is_assignable_bounded(&actual, &expected, &parameter.span) {
                    self.type_error(
                        &parameter.span,
                        format!(
                            "default initializer has type `{}`, which is not assignable to parameter `{}` of type `{}`",
                            type_label(&actual),
                            parameter.name,
                            type_label(&expected)
                        ),
                        DiagnosticCode::TypeMismatch,
                    );
                }
            }
            if let Some(annotation) = &parameter.annotation {
                self.check_type(annotation, &parameter.span);
                let parameter_type = if parameter.optional && parameter.default.is_none() {
                    Type::Union(vec![annotation.clone(), Type::Undefined])
                } else {
                    annotation.clone()
                };
                scope.insert(parameter.name.clone(), parameter_type);
            } else {
                scope.insert(parameter.name.clone(), Type::Unknown);
            }
        }
        for local in &function.locals {
            self.check_variable_in_scope(local, &scope);
            scope.insert(
                local.name.clone(),
                local.annotation.clone().unwrap_or(Type::Unknown),
            );
        }
        self.check_function_body_expressions(&function.body, &scope);
        if let Some(return_type) = &function.return_type {
            self.check_type(return_type, &function.span);
            for returned in &function.returns {
                if returned.is_empty() {
                    continue;
                }
                self.check_function_call(returned, &scope, &function.span);
                self.check_direct_property_access(returned, &scope, &function.span);
                self.check_arithmetic_operators(returned, &scope, &function.span);
                let actual = self.infer_expression(returned, &scope);
                if !self.is_assignable_bounded(&actual, return_type, &function.span) {
                    self.type_error(
                        &function.span,
                        format!(
                            "return expression has type `{}`, which is not assignable to `{}`",
                            type_label(&actual),
                            type_label(return_type)
                        ),
                        DiagnosticCode::ReturnTypeMismatch,
                    );
                }
            }
        }
        self.type_parameters = previous_parameters;
    }

    pub(super) fn check_function_body_expressions(
        &mut self,
        items: &[FunctionBodyItem],
        scope: &BTreeMap<String, Type>,
    ) {
        for item in items {
            let (tokens, span) = match item {
                FunctionBodyItem::Expression { tokens, span }
                | FunctionBodyItem::Throw { tokens, span } => (tokens, span),
                FunctionBodyItem::If(statement) => {
                    self.check_direct_function_if(statement, scope);
                    continue;
                }
                _ => continue,
            };
            self.check_direct_runtime_expression(tokens, scope, span);
        }
    }

    pub(super) fn check_direct_function_if(
        &mut self,
        statement: &FunctionIfStatement,
        scope: &BTreeMap<String, Type>,
    ) {
        self.check_direct_runtime_expression(&statement.test, scope, &statement.span);
        self.check_function_body_expressions(&statement.consequent, scope);
        match &statement.alternate {
            Some(FunctionElseBranch::Braced(alternate)) => {
                self.check_function_body_expressions(alternate, scope);
            }
            Some(FunctionElseBranch::ElseIf(alternate)) => {
                self.check_direct_function_if(alternate, scope);
            }
            None => {}
        }
    }

    pub(super) fn check_direct_runtime_expression(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        if tokens.is_empty() {
            return;
        }
        self.check_function_call(tokens, scope, span);
        self.check_direct_property_access(tokens, scope, span);
        self.check_arithmetic_operators(tokens, scope, span);
    }

    pub(super) fn check_type(&mut self, value: &Type, span: &SourceSpan) {
        match value {
            Type::Named { name, arguments } => {
                if let Some(definition) = self.types.get(name).cloned() {
                    self.check_type_arguments(name, arguments, &definition, span);
                } else if !self.type_parameters.contains(name) {
                    self.type_error(
                        span,
                        format!("cannot find type `{name}`"),
                        DiagnosticCode::UnknownType,
                    );
                }
            }
            Type::Array(value) => self.check_type(value, span),
            Type::Tuple(values) | Type::Union(values) | Type::Intersection(values) => {
                for value in values {
                    self.check_type(value, span);
                }
            }
            Type::Record(fields) => {
                for field in fields {
                    self.check_type(&field.value, &field.span);
                }
            }
            _ => {}
        }
    }

    pub(super) fn check_type_parameters(&mut self, parameters: &[TypeParameter]) {
        let mut saw_default = false;
        for parameter in parameters {
            if !self.type_parameters.insert(parameter.name.clone()) {
                self.type_error(
                    &parameter.span,
                    format!("duplicate type parameter `{}`", parameter.name),
                    DiagnosticCode::DuplicateDeclaration,
                );
            }
            if saw_default && parameter.default.is_none() {
                self.type_error(
                    &parameter.span,
                    format!(
                        "required type parameter `{}` cannot follow a defaulted type parameter",
                        parameter.name
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
            if parameter.default.is_some() {
                saw_default = true;
            }
            if let Some(constraint) = &parameter.constraint {
                self.check_type(constraint, &parameter.span);
            }
            if let Some(default) = &parameter.default {
                self.check_type(default, &parameter.span);
                if let Some(constraint) = &parameter.constraint {
                    if !self.is_assignable_bounded(default, constraint, &parameter.span) {
                        self.type_error(
                            &parameter.span,
                            format!(
                                "default type `{}` does not satisfy constraint `{}` for `{}`",
                                type_label(default),
                                type_label(constraint),
                                parameter.name,
                            ),
                            DiagnosticCode::TypeMismatch,
                        );
                    }
                }
            }
        }
    }

    pub(super) fn check_type_arguments(
        &mut self,
        name: &str,
        arguments: &[Type],
        definition: &TypeDefinition,
        span: &SourceSpan,
    ) {
        let required = definition
            .parameters
            .iter()
            .filter(|parameter| parameter.default.is_none())
            .count();
        if arguments.len() < required || arguments.len() > definition.parameters.len() {
            self.type_error(
                span,
                format!(
                    "type `{name}` requires {required} to {} type argument(s), got {}",
                    definition.parameters.len(),
                    arguments.len(),
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
        for argument in arguments {
            self.check_type(argument, span);
        }
        let Some(arguments) = complete_type_arguments(&definition.parameters, arguments) else {
            return;
        };
        let substitutions = definition
            .parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .zip(arguments)
            .collect::<BTreeMap<_, _>>();
        for parameter in &definition.parameters {
            let Some(constraint) = &parameter.constraint else {
                continue;
            };
            let actual = substitutions
                .get(&parameter.name)
                .expect("completed generic arguments contain every parameter");
            let expected = substitute_type(constraint, &substitutions);
            if !self.is_assignable_bounded(actual, &expected, span) {
                self.type_error(
                    span,
                    format!(
                        "type argument `{}` does not satisfy constraint `{}` for `{}`",
                        type_label(actual),
                        type_label(&expected),
                        parameter.name,
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    pub(super) fn type_error(&mut self, span: &SourceSpan, message: String, code: DiagnosticCode) {
        if self.enforce_types {
            self.diagnostics
                .push(Diagnostic::error(code, span.clone(), message));
        }
    }
}
