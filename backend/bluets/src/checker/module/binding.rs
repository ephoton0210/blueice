// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration binding and structural validation for one module.

use super::*;

mod checking_flags;
mod classes;
mod diagnostics;
mod enums;
mod functions;
mod generators;
pub(in crate::checker::module) use generators::GeneratorContext;
mod modules;
mod names;
mod predicates;
mod return_types;
mod standard_library;

use modules::specialize_imported_class_type;
mod namespaces;
pub(in crate::checker::module) use functions::promise_value_type;
pub(crate) use namespaces::{ExportedValue, NamespaceExport, NamespaceMembers};
pub(in crate::checker::module) use nested_functions::{async_result, declared_function_type};
mod nested_functions;
pub(in crate::checker) use classes::{class_export, class_instance_type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StructuredTermination {
    Terminates,
    FallsThrough,
    Opaque,
}

impl<'a> ModuleChecker<'a> {
    pub(crate) fn new(
        project: &'a Project,
        module: &'a Module,
        exports: &'a ProjectExports,
        ambient: Option<&'a AmbientDeclarations>,
        namespace_exports: &'a NamespaceExports,
        policy: CheckerPolicy,
        max_type_expansions: usize,
    ) -> Self {
        Self {
            checking: policy
                .checking
                .unwrap_or_else(crate::CheckingOptions::legacy),
            explicit_checking: policy.checking.is_some(),
            target: policy.target,
            project,
            scopes: None,
            flow: None,
            module,
            exports,
            ambient,
            namespace_exports,
            pending_imports: BTreeSet::new(),
            module_namespace_imports: BTreeSet::new(),
            enforce_types: policy.enforce_types,
            require_declared_global_calls: policy.require_declared_global_calls,
            define_class_fields: policy.define_class_fields,
            isolated_modules: policy.isolated_modules,
            module_kind: policy.module_kind,
            es_module_interop: policy.es_module_interop,
            jsx_mode: policy.jsx,
            experimental_decorators: policy.experimental_decorators,
            jsx_factory: policy.jsx_factory.clone(),
            jsx_fragment_factory: policy.jsx_fragment_factory.clone(),
            jsx_pragmas: crate::jsx::Pragmas::of(&module.source),
            checked_jsx_elements: BTreeSet::new(),
            checked_nested_functions: BTreeSet::new(),
            async_context: None,
            generator_context: None,
            constructor_readonly_fields: None,
            access_class: None,
            enum_members: BTreeMap::new(),
            const_enums: BTreeSet::new(),
            type_only_enums: BTreeSet::new(),
            ambient_const_enums: BTreeSet::new(),
            enum_evaluations: Vec::new(),
            restricted_member_names: BTreeSet::new(),
            namespace_path: String::new(),
            namespaces: BTreeMap::new(),
            type_only_namespaces: BTreeSet::new(),
            annotated_names: BTreeSet::new(),
            diagnostics: Vec::new(),
            symbols: Vec::new(),
            types: BTreeMap::new(),
            values: BTreeMap::new(),
            library_values: BTreeSet::new(),
            functions: BTreeMap::new(),
            class_constructors: BTreeMap::new(),
            type_only_classes: BTreeSet::new(),
            function_implementations: BTreeSet::new(),
            type_parameters: BTreeSet::new(),
            allowed_tuple_spread_parameters: BTreeMap::new(),
            max_type_expansions,
            strict_catch_unknown: false,
            record_spread_inference_failure: Cell::new(None),
            generic_inference_failure: Cell::new(None),
            return_inference: Default::default(),
        }
    }

    pub(crate) fn bind(&mut self) {
        self.bind_declarations();
        self.bind_ambient_declarations();
        self.bind_standard_library();
        self.infer_module_return_signatures();
        self.validate_function_overloads();
        self.validate_default_exports();
        self.validate_value_exports();
        self.validate_module_system();
    }

    /// Binds what the module's (or a namespace body's) own declarations declare.
    pub(super) fn bind_declarations(&mut self) {
        self.enum_evaluations = crate::enum_eval::evaluate_enums(self.module);
        let mut enum_index = 0usize;
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Enum(declaration) => {
                    let evaluated = self.enum_evaluations[enum_index].clone();
                    enum_index += 1;
                    self.bind_enum(declaration, &evaluated);
                }
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
                    // An interface named like a class of this scope merges into
                    // the class's type instead of defining one of its own.
                    let merges_into_class = self.module.declarations.iter().any(|other| {
                        matches!(other, Declaration::Class(class) if class.name == interface.name)
                    });
                    if !merges_into_class
                        && !self
                            .types
                            .get(&interface.name)
                            .is_some_and(|definition| definition.kind == TypeDefinitionKind::Class)
                    {
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
                }
                Declaration::Variable(variable) => {
                    if variable.annotation.is_some() {
                        self.annotated_names.insert(variable.name.clone());
                    }
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
                Declaration::Class(class) => {
                    self.bind_class(class);
                    // Only a class whose every member has a structured form
                    // (constructors and methods) is admitted, so emitted
                    // JavaScript never carries unerased TypeScript.
                    if !classes::class_is_fully_structured(class) {
                        self.diagnostics.push(self.class_shape_counterpart(class, Diagnostic::error(
                            DiagnosticCode::UnsupportedSyntax,
                            class.span.clone(),
                            "a class member other than a constructor, method, field or accessor \
                             (a computed, generator or `accessor` member) is not supported yet",
                        )));
                    }
                }
                Declaration::Namespace(namespace) => self.bind_namespace(namespace),
                Declaration::Raw(_) => {}
            }
        }
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

    pub(super) fn insert_type(
        &mut self,
        name: &str,
        definition: TypeDefinition,
        span: SourceSpan,
        kind: SymbolKind,
        exported: bool,
    ) {
        if definition.kind == TypeDefinitionKind::Interface {
            if let Some(existing) = self
                .types
                .get(name)
                .filter(|existing| existing.kind == TypeDefinitionKind::Interface)
                .cloned()
            {
                let members = |value: &Type| match value {
                    Type::Record(fields) | Type::CallableRecord { fields, .. } => fields.clone(),
                    _ => Vec::new(),
                };
                let old_fields = members(&existing.value);
                for field in members(&definition.value) {
                    if let Some(old) = old_fields.iter().find(|old| old.name == field.name) {
                        if !matches!(
                            field.value,
                            Type::Function { .. } | Type::GenericFunction { .. }
                        ) && old.value != field.value
                        {
                            self.typescript_type_error(
                                &field.span,
                                format!(
                                    "subsequent interface property `{}` has a different type",
                                    field.name
                                ),
                                DiagnosticCode::TypeMismatch,
                                2717,
                                vec![
                                    field.name.clone(),
                                    crate::diagnostic::type_text::render_in(
                                        &old.value,
                                        self.project,
                                    ),
                                    crate::diagnostic::type_text::render_in(
                                        &field.value,
                                        self.project,
                                    ),
                                ],
                            );
                        }
                    }
                }
                self.types.get_mut(name).unwrap().value =
                    merge_interface_values(&existing.value, &definition.value);
                return;
            }
        }
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
        self.diagnostics.push(self.duplicate_counterpart(
            name,
            Diagnostic::error(
                DiagnosticCode::DuplicateDeclaration,
                span,
                format!("duplicate declaration of `{name}`"),
            ),
        ));
    }

    pub(crate) fn check_types(&mut self) {
        self.validate_class_heritage_cycles();
        self.bind_inherited_class_instance_methods();
        self.bind_inherited_class_static_methods();
        self.bind_inherited_class_constructors();
        self.collect_restricted_member_names();
        let mut enum_index = 0usize;
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Enum(declaration) => {
                    let evaluated = self.enum_evaluations[enum_index].clone();
                    enum_index += 1;
                    self.check_enum(declaration, &evaluated);
                }
                Declaration::TypeAlias(alias) => {
                    let previous_spreads = self.allowed_tuple_spread_parameters.clone();
                    self.allowed_tuple_spread_parameters = alias
                        .type_parameters
                        .iter()
                        .filter(|parameter| {
                            matches!(parameter.constraint, Some(Type::Array(_) | Type::Tuple(_)))
                        })
                        .map(|parameter| {
                            (
                                parameter.name.clone(),
                                parameter.constraint.clone().expect("filtered constraint"),
                            )
                        })
                        .collect();
                    self.check_type_with_parameters(
                        &alias.value,
                        &alias.span,
                        &alias.type_parameters,
                    );
                    self.allowed_tuple_spread_parameters = previous_spreads;
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
                    for signature in &interface.signatures {
                        self.check_type_with_parameters(
                            &signature.function_type(),
                            &signature.span,
                            &interface.type_parameters,
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
                Declaration::Class(original) => {
                    let inferred = self.class_with_inferred_returns(original);
                    let class = &inferred;
                    let previous_parameters = self.type_parameters.clone();
                    self.check_type_parameters(&class.type_parameters);
                    self.validate_class_heritage_name(class);
                    self.validate_class_constructor_group(class);
                    self.validate_class_method_groups(class);
                    self.validate_class_method_overrides(class);
                    self.validate_class_accessors(class);
                    self.validate_class_visibility(class);
                    self.with_class_access(class, |checker| {
                        checker.validate_class_fields(class);
                        checker.check_class_constructor_bodies(class);
                        checker.check_class_method_bodies(class);
                        checker.check_class_static_blocks(class);
                        checker.check_class_decorators(class);
                    });
                    self.type_parameters = previous_parameters;
                }
                Declaration::Import(_)
                | Declaration::TypeExport(_)
                | Declaration::DefaultExport(_)
                | Declaration::ValueExport(_) => {}
                Declaration::Namespace(namespace) => self.check_namespace(namespace),
                Declaration::Raw(raw) => {
                    let scope = self.values.clone();
                    self.check_direct_runtime_expression(&raw.tokens, &scope, &raw.span);
                }
            }
        }
        self.report_return_inference_failures();
        if let Some(span) = self.generic_inference_failure.take() {
            self.type_error(
                &span,
                format!(
                    "contextual generic inference exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            );
        }
        if let Some((start, end, failure)) = self.record_spread_inference_failure.take() {
            let (message, code) = match failure {
                RecordSpreadFailure::ResourceLimit => (
                    format!(
                        "record spread exceeds the {}-type-expansion inference limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                ),
                RecordSpreadFailure::UnprovenSource => (
                    "record spread source must have a provable record type".to_string(),
                    DiagnosticCode::TypeMismatch,
                ),
            };
            self.type_error(&SourceSpan::new(&self.module.id, start, end), message, code);
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
                    kind: TypeDefinitionKind::Interface | TypeDefinitionKind::LibraryInterface,
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
            let PropertyType::Found {
                value: inherited, ..
            } = inherited
            else {
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
        if variable.annotation.is_none() && !variable.initializer.is_empty() && !variable.declared {
            let inferred = self
                .alias_function_signatures(variable, &scope)
                .unwrap_or_else(|| self.infer_variable_type(variable, &scope));
            self.values.insert(variable.name.clone(), inferred.clone());
            self.refresh_variable_symbol(variable, inferred);
        }
    }

    pub(super) fn check_variable_in_scope(
        &mut self,
        variable: &crate::parser::VariableDeclaration,
        scope: &BTreeMap<String, Type>,
    ) {
        if let Some(annotation) = &variable.annotation {
            self.infer_in_context(&variable.initializer, scope, annotation);
        }
        if !variable.initializer.is_empty() && !variable.declared {
            self.check_direct_runtime_expression(&variable.initializer, scope, &variable.span);
        }
        let Some(annotation) = &variable.annotation else {
            return;
        };
        self.check_type(annotation, &variable.span);
        if variable.initializer.is_empty() || variable.declared || !self.type_is_bound(annotation) {
            return;
        }
        let inferred = self.infer_in_context(&variable.initializer, scope, annotation);
        if !self.is_assignable_bounded(&inferred, annotation, &variable.span) {
            self.assignment_error(
                &variable.span,
                format!(
                    "initializer has type `{}`, which is not assignable to `{}`",
                    type_label(&inferred),
                    type_label(annotation)
                ),
                DiagnosticCode::TypeMismatch,
                &inferred,
                annotation,
            );
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
        self.check_namespace_value_use(tokens, span);
        self.check_jsx_elements(tokens, scope);
        self.check_yield_expressions(tokens, scope, span);
        let before_arrows = self.diagnostics.len();
        self.contextualize_call_arguments(tokens, scope);
        self.check_nested_functions_in(tokens, scope);
        self.dedupe_diagnostics_since(before_arrows);
        self.check_await_context(tokens, span);
        if tokens
            .iter()
            .filter(|token| token.is("[") || token.is("{"))
            .count()
            > MAX_LITERAL_INFERENCE_CONTAINERS
        {
            self.type_error(
                span,
                format!(
                    "expression exceeds its {MAX_LITERAL_INFERENCE_CONTAINERS}-container inference limit"
                ),
                DiagnosticCode::ResourceLimit,
            );
            return;
        }
        if tokens
            .iter()
            .filter(|token| INFERRED_LOGICAL_ASSIGNMENT_OPERATORS.contains(&token.text.as_str()))
            .count()
            > MAX_LOGICAL_ASSIGNMENT_INFERENCE_OPERATORS
        {
            self.type_error(
                span,
                format!(
                    "expression exceeds its {MAX_LOGICAL_ASSIGNMENT_INFERENCE_OPERATORS}-logical assignment inference limit"
                ),
                DiagnosticCode::ResourceLimit,
            );
            return;
        }
        if tokens
            .iter()
            .filter(|token| INFERRED_LOGICAL_EXPRESSION_OPERATORS.contains(&token.text.as_str()))
            .count()
            > MAX_LOGICAL_EXPRESSION_INFERENCE_OPERATORS
        {
            self.type_error(
                span,
                format!(
                    "expression exceeds its {MAX_LOGICAL_EXPRESSION_INFERENCE_OPERATORS}-logical expression inference limit"
                ),
                DiagnosticCode::ResourceLimit,
            );
            return;
        }
        if tokens.iter().filter(|token| token.is(".")).count() > self.max_type_expansions {
            self.type_error(
                span,
                format!(
                    "member access exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            );
            return;
        }
        self.check_type_only_class_value_uses(tokens, scope, span);
        if self.check_optional_property_read(tokens, scope, span) {
            return;
        }
        self.check_class_constructions_in_expression(tokens, scope, span);
        self.check_bound_class_calls_in_expression(tokens, scope, span);
        self.check_flow_in_subjects(tokens, scope);
        self.check_function_call(tokens, scope, span);
        self.check_flow_calls(tokens, scope, span);
        self.check_member_calls_in_expression(tokens, scope, span);
        self.check_direct_property_access(tokens, scope, span);
        self.check_member_assignment(tokens, scope, span);
        self.check_variable_assignment(tokens, scope, span);
        self.check_restricted_member_access(tokens, scope, span);
        self.check_enum_index_uses(tokens, scope, span);
        self.check_const_enum_uses(tokens, scope, span);
        self.check_type_only_enum_value_uses(tokens, scope, span);
        self.check_arithmetic_operators(tokens, scope, span);
    }
}
