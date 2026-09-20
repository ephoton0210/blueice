// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Name binding and deterministic, deliberately bounded type checking.

use crate::compiler::{is_declaration_module, Project};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{
    Declaration, FunctionDeclaration, InterfaceDeclaration, Module, Parameter, TypeField,
    TypeParameter,
};
use crate::syntax::{Token, TokenKind};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub use crate::parser::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Import,
    TypeAlias,
    Interface,
    Variable,
    Function,
}

/// A source-level binding.  It has no runtime identity and is suitable for
/// diagnostics, declarations, source maps, and future debugger metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub module: String,
    pub span: SourceSpan,
    pub exported: bool,
    pub value_type: Option<Type>,
}

#[derive(Debug, Clone)]
pub struct CheckedModule {
    pub module: Module,
    pub symbols: Vec<Symbol>,
}

#[derive(Debug, Clone)]
pub struct CheckedProject {
    pub modules: BTreeMap<String, CheckedModule>,
}

/// A locally bound type declaration.  Keeping its parameters alongside its
/// body lets the checker instantiate erased generic aliases and interfaces at
/// their use sites without making a type parameter visible outside its own
/// declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeDefinition {
    kind: TypeDefinitionKind,
    parameters: Vec<TypeParameter>,
    value: Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeDefinitionKind {
    Alias,
    Interface,
}

#[derive(Debug, Clone)]
struct FunctionSignature {
    parameters: Vec<Parameter>,
    type_parameters: Vec<TypeParameter>,
    return_type: Type,
}

struct TypeExpansionBudget {
    remaining: usize,
    exhausted: bool,
}

impl TypeExpansionBudget {
    fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            exhausted: false,
        }
    }

    fn consume(&mut self) -> bool {
        if let Some(remaining) = self.remaining.checked_sub(1) {
            self.remaining = remaining;
            true
        } else {
            self.exhausted = true;
            false
        }
    }
}

/// Rechecks the requested modules while retaining checker output for modules
/// that the incremental project graph proved unaffected. The caller must only
/// supply a previous project checked under the same compiler policy and must
/// include every reverse dependency of a changed module in `rechecked`.
pub(crate) fn check_incremental(
    project: &Project,
    enforce_types: bool,
    previous: Option<&CheckedProject>,
    rechecked: &BTreeSet<String>,
    max_type_expansions: usize,
) -> (CheckedProject, Vec<Diagnostic>) {
    let mut diagnostics = declaration_module_diagnostics(project);
    let exported_types = exported_types(project);
    let mut checked_modules = BTreeMap::new();

    for (module_id, module) in &project.modules {
        if !rechecked.contains(module_id) {
            if let Some(previous) = previous.and_then(|previous| previous.modules.get(module_id)) {
                checked_modules.insert(module_id.clone(), previous.clone());
                continue;
            }
        }
        let mut checker = ModuleChecker::new(
            project,
            module,
            &exported_types,
            enforce_types,
            max_type_expansions,
        );
        checker.bind();
        if enforce_types {
            checker.check_types();
        }
        diagnostics.extend(checker.diagnostics);
        checked_modules.insert(
            module_id.clone(),
            CheckedModule {
                module: module.clone(),
                symbols: checker.symbols,
            },
        );
    }
    (
        CheckedProject {
            modules: checked_modules,
        },
        diagnostics,
    )
}

fn declaration_module_diagnostics(project: &Project) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (module_id, module) in &project.modules {
        for declaration in &module.declarations {
            if let Declaration::Import(import) = declaration {
                if !import.type_only
                    && project
                        .resolutions
                        .get(&(module_id.clone(), import.specifier.clone()))
                        .is_some_and(|resolved| is_declaration_module(resolved))
                {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::InvalidDeclarationFile,
                        import.span.clone(),
                        format!(
                            "value import `{}` resolves to declaration module; declaration modules are type-only",
                            import.specifier
                        ),
                    ));
                }
            }
        }
        if !is_declaration_module(module_id) {
            continue;
        }
        for declaration in &module.declarations {
            let runtime_declaration = match declaration {
                Declaration::Import(import) => !import.type_only,
                Declaration::Variable(variable) => !variable.declared,
                Declaration::Function(function) => !function.declared && !function.overload,
                Declaration::Raw(_) => true,
                Declaration::DefaultExport(_)
                | Declaration::ValueExport(_)
                | Declaration::TypeExport(_)
                | Declaration::TypeAlias(_)
                | Declaration::Interface(_) => false,
            };
            if runtime_declaration {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::InvalidDeclarationFile,
                    declaration.span().clone(),
                    "declaration module contains a runtime declaration",
                ));
            }
        }
    }
    diagnostics
}

fn exported_types(project: &Project) -> BTreeMap<String, BTreeMap<String, TypeDefinition>> {
    let mut modules = BTreeMap::new();
    for (id, module) in &project.modules {
        // An exported interface can inherit a private, local parent. Its
        // exported definition must therefore carry the inherited shape rather
        // than make consumers resolve an unimportable implementation detail.
        let declared = local_type_definitions(module);
        let mut values = BTreeMap::new();
        for declaration in &module.declarations {
            match declaration {
                Declaration::TypeAlias(alias) if alias.exported => {
                    values.insert(
                        alias.name.clone(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::Alias,
                            parameters: alias.type_parameters.clone(),
                            value: alias.value.clone(),
                        },
                    );
                }
                Declaration::Interface(interface) if interface.exported => {
                    values.insert(
                        interface.name.clone(),
                        TypeDefinition {
                            kind: TypeDefinitionKind::Interface,
                            parameters: interface.type_parameters.clone(),
                            value: exported_interface_value(interface, &declared),
                        },
                    );
                }
                _ => {}
            }
        }
        modules.insert(id.clone(), values);
    }
    // Type-only re-exports are static edges, but they still contribute to the
    // public type surface consumed by another module.  Resolve this small
    // fixed point without executing module code; the graph is already closed
    // and bounded by the project resolver.
    for _ in 0..project.modules.len() {
        let mut changed = false;
        for (module_id, module) in &project.modules {
            let mut additions = BTreeMap::new();
            for declaration in &module.declarations {
                let Declaration::TypeExport(export) = declaration else {
                    continue;
                };
                let Some(specifier) = &export.specifier else {
                    continue;
                };
                let Some(source_id) = project
                    .resolutions
                    .get(&(module_id.clone(), specifier.clone()))
                else {
                    continue;
                };
                let Some(source_types) = modules.get(source_id) else {
                    continue;
                };
                for binding in &export.bindings {
                    if binding == "*" {
                        additions.extend(source_types.clone());
                        continue;
                    }
                    let (local, exported) = binding
                        .split_once(" as ")
                        .map_or((binding.as_str(), binding.as_str()), |(local, exported)| {
                            (local, exported)
                        });
                    if let Some(value) = source_types.get(local) {
                        additions.insert(exported.to_string(), value.clone());
                    }
                }
            }
            let target = modules.entry(module_id.clone()).or_default();
            for (name, value) in additions {
                if target.get(&name) != Some(&value) {
                    target.insert(name, value);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    modules
}

fn local_type_definitions(module: &Module) -> BTreeMap<String, TypeDefinition> {
    module
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::TypeAlias(alias) => Some((
                alias.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Alias,
                    parameters: alias.type_parameters.clone(),
                    value: alias.value.clone(),
                },
            )),
            Declaration::Interface(interface) => Some((
                interface.name.clone(),
                TypeDefinition {
                    kind: TypeDefinitionKind::Interface,
                    parameters: interface.type_parameters.clone(),
                    value: interface_value(interface),
                },
            )),
            _ => None,
        })
        .collect()
}

fn exported_interface_value(
    interface: &InterfaceDeclaration,
    declared: &BTreeMap<String, TypeDefinition>,
) -> Type {
    let mut active = HashSet::new();
    let heritage = interface
        .heritage
        .iter()
        .map(|parent| expand_exported_heritage(parent, declared, &mut active))
        .collect::<Vec<_>>();
    interface_value_with_heritage(&heritage, &interface.fields)
}

fn expand_exported_heritage(
    value: &Type,
    declared: &BTreeMap<String, TypeDefinition>,
    active: &mut HashSet<String>,
) -> Type {
    match value {
        Type::Named { name, arguments } => {
            let Some(definition) = declared.get(name) else {
                return value.clone();
            };
            let Some(arguments) = complete_type_arguments(&definition.parameters, arguments) else {
                return value.clone();
            };
            let key = format!("heritage:{}", type_identity(value));
            if !active.insert(key.clone()) {
                return value.clone();
            }
            let substitutions = type_parameter_substitutions(&definition.parameters, arguments);
            let expanded = substitute_type(&definition.value, &substitutions);
            let result = expand_exported_heritage(&expanded, declared, active);
            active.remove(&key);
            result
        }
        Type::Intersection(parts) => Type::Intersection(
            parts
                .iter()
                .map(|part| expand_exported_heritage(part, declared, active))
                .collect(),
        ),
        _ => value.clone(),
    }
}

struct ModuleChecker<'a> {
    project: &'a Project,
    module: &'a Module,
    exported_types: &'a BTreeMap<String, BTreeMap<String, TypeDefinition>>,
    enforce_types: bool,
    diagnostics: Vec<Diagnostic>,
    symbols: Vec<Symbol>,
    types: BTreeMap<String, TypeDefinition>,
    values: BTreeMap<String, Type>,
    functions: BTreeMap<String, Vec<FunctionSignature>>,
    function_implementations: BTreeSet<String>,
    type_parameters: BTreeSet<String>,
    max_type_expansions: usize,
}

impl<'a> ModuleChecker<'a> {
    fn new(
        project: &'a Project,
        module: &'a Module,
        exported_types: &'a BTreeMap<String, BTreeMap<String, TypeDefinition>>,
        enforce_types: bool,
        max_type_expansions: usize,
    ) -> Self {
        Self {
            project,
            module,
            exported_types,
            enforce_types,
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

    fn bind(&mut self) {
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
        self.validate_function_overloads();
        self.validate_default_exports();
        self.validate_value_exports();
    }

    fn validate_default_exports(&mut self) {
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

    fn validate_value_exports(&mut self) {
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

    fn validate_function_overloads(&mut self) {
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

    fn bind_import(&mut self, import: &crate::parser::ImportDeclaration) {
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

    fn bind_type_export(&mut self, export: &crate::parser::TypeExportDeclaration) {
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

    fn insert_type(
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

    fn insert_value(
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

    fn duplicate(&mut self, name: &str, span: SourceSpan) {
        self.diagnostics.push(Diagnostic::error(
            DiagnosticCode::DuplicateDeclaration,
            span,
            format!("duplicate declaration of `{name}`"),
        ));
    }

    fn check_types(&mut self) {
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
                | Declaration::ValueExport(_)
                | Declaration::Raw(_) => {}
            }
        }
    }

    fn check_type_with_parameters(
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

    fn check_interface_heritage(
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

    fn check_inherited_field_compatibility(
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

    fn check_variable(&mut self, variable: &crate::parser::VariableDeclaration) {
        let scope = self.values.clone();
        self.check_variable_in_scope(variable, &scope);
    }

    fn check_variable_in_scope(
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
        let inferred = self.infer_expression(&variable.initializer, scope);
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

    fn check_function(&mut self, function: &FunctionDeclaration) {
        let mut scope = self.values.clone();
        let previous_parameters = self.type_parameters.clone();
        self.check_type_parameters(&function.type_parameters);
        for parameter in &function.parameters {
            if let Some(annotation) = &parameter.annotation {
                self.check_type(annotation, &parameter.span);
                scope.insert(parameter.name.clone(), annotation.clone());
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

    fn check_type(&mut self, value: &Type, span: &SourceSpan) {
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

    fn check_type_parameters(&mut self, parameters: &[TypeParameter]) {
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

    fn check_type_arguments(
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

    fn type_error(&mut self, span: &SourceSpan, message: String, code: DiagnosticCode) {
        if self.enforce_types {
            self.diagnostics
                .push(Diagnostic::error(code, span.clone(), message));
        }
    }

    fn infer_expression(&self, tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
        let tokens = strip_outer_parentheses(tokens);
        if let Some(call) =
            direct_call_parts(tokens).filter(|call| split_call_arguments(call.arguments).is_some())
        {
            if let Some(signatures) = self.functions.get(&call.callee.text) {
                let explicit = call.generic.then(|| {
                    self.module
                        .generic_call_type_arguments
                        .get(&call.callee.start)
                        .expect("parsed generic call has recorded type arguments")
                        .as_slice()
                });
                return self.infer_function_call(signatures, call.arguments, scope, explicit);
            }
            return scope
                .get(&call.callee.text)
                .cloned()
                .unwrap_or(Type::Unknown);
        }
        if let Some((_, consequent, alternate)) = conditional_expression_parts(tokens) {
            return merge_conditional_branch_types(
                self.infer_expression(consequent, scope),
                self.infer_expression(alternate, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["||"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_boolean_logical_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["&&"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_boolean_logical_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if top_level_binary_parts(
            tokens,
            &["===", "!==", "==", "!=", "<", ">", "<=", ">="],
            |start| self.module.generic_call_type_arguments.contains_key(&start),
        )
        .is_some()
        {
            return Type::Boolean;
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["+", "-"], |start| {
                self.module.generic_call_type_arguments.contains_key(&start)
            })
        {
            return infer_additive_expression(
                operator,
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        if let Some((left, _, right)) = top_level_binary_parts(tokens, &["*", "/", "%"], |start| {
            self.module.generic_call_type_arguments.contains_key(&start)
        }) {
            return infer_numeric_binary_expression(
                self.infer_expression(left, scope),
                self.infer_expression(right, scope),
            );
        }
        let Some(first) = tokens.first() else {
            return Type::Undefined;
        };
        if matches!(first.text.as_str(), "!") {
            return Type::Boolean;
        }
        if matches!(first.text.as_str(), "+" | "-" | "~") {
            return Type::Number;
        }
        if first.kind == TokenKind::String || first.kind == TokenKind::Template {
            return Type::String;
        }
        if first.kind == TokenKind::Number {
            return Type::Number;
        }
        match first.text.as_str() {
            "true" | "false" => Type::Boolean,
            "null" => Type::Null,
            "undefined" => Type::Undefined,
            "[" => infer_array(tokens, scope),
            "{" => infer_record(tokens, scope),
            _ if first.kind == TokenKind::Identifier => {
                if tokens.get(1).is_some_and(|token| token.is("."))
                    && tokens
                        .get(2)
                        .is_some_and(|token| token.kind == TokenKind::Identifier)
                {
                    let base = scope.get(&first.text).cloned().unwrap_or(Type::Unknown);
                    let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
                    return match property_type(
                        &base,
                        &tokens[2].text,
                        &self.types,
                        &mut HashSet::new(),
                        &mut budget,
                    ) {
                        PropertyType::Found(value) => value,
                        PropertyType::Missing
                        | PropertyType::Indeterminate
                        | PropertyType::Exhausted => Type::Unknown,
                    };
                }
                scope.get(&first.text).cloned().unwrap_or(Type::Unknown)
            }
            _ => Type::Unknown,
        }
    }

    fn infer_function_call(
        &self,
        signatures: &[FunctionSignature],
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        explicit_type_arguments: Option<&[Type]>,
    ) -> Type {
        let Some(arguments) = split_call_arguments(tokens) else {
            return Type::Unknown;
        };
        let actuals = arguments
            .iter()
            .map(|argument| self.infer_expression(argument, scope))
            .collect::<Vec<_>>();
        let Ok(Some(signature)) =
            self.select_function_signature(signatures, &actuals, explicit_type_arguments)
        else {
            return Type::Unknown;
        };
        let substitutions =
            function_call_substitutions(signature, &actuals, explicit_type_arguments)
                .expect("selected function signature has valid substitutions");
        substitute_type(&signature.return_type, &substitutions)
    }

    fn check_function_call(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let Some(call) = direct_call_parts(tokens) else {
            return;
        };
        let Some(signatures) = self.functions.get(&call.callee.text).cloned() else {
            return;
        };
        let Some(arguments) = split_call_arguments(call.arguments) else {
            return;
        };
        let actuals = arguments
            .iter()
            .map(|argument| {
                self.check_direct_property_access(argument, scope, span);
                self.infer_expression(argument, scope)
            })
            .collect::<Vec<_>>();
        let explicit = call.generic.then(|| {
            self.module
                .generic_call_type_arguments
                .get(&call.callee.start)
                .expect("parsed generic call has recorded type arguments")
                .as_slice()
        });
        let selected = match self.select_function_signature(&signatures, &actuals, explicit) {
            Ok(selected) => selected.cloned(),
            Err(()) => {
                self.type_error(
                    span,
                    format!(
                        "overload selection exceeds the {} generic-expansion limit",
                        self.max_type_expansions
                    ),
                    DiagnosticCode::ResourceLimit,
                );
                return;
            }
        };
        if signatures.len() > 1 && selected.is_none() {
            self.type_error(
                span,
                format!(
                    "no overload of function {} accepts the supplied argument types",
                    call.callee.text
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        }
        let Some(signature) = selected.or_else(|| signatures.first().cloned()) else {
            return;
        };
        let required = signature
            .parameters
            .iter()
            .filter(|parameter| !parameter.optional)
            .count();
        if arguments.len() < required || arguments.len() > signature.parameters.len() {
            self.type_error(
                span,
                format!(
                    "function {} expects {} to {} argument(s), got {}",
                    call.callee.text,
                    required,
                    signature.parameters.len(),
                    arguments.len()
                ),
                DiagnosticCode::TypeMismatch,
            );
            return;
        }
        let substitutions = if let Some(explicit) = explicit {
            let Some(substitutions) =
                self.check_explicit_function_type_arguments(&signature, explicit, span)
            else {
                return;
            };
            substitutions
        } else {
            let substitutions = infer_call_substitutions(&signature, &actuals);
            self.check_call_type_parameter_constraints(
                &signature,
                &substitutions,
                span,
                "inferred type",
            );
            substitutions
        };
        for (index, (parameter, actual)) in signature.parameters.iter().zip(actuals).enumerate() {
            let expected = parameter_expected_type(parameter, &substitutions);
            if !self.is_assignable_bounded(&actual, &expected, span) {
                self.type_error(
                    span,
                    format!(
                        "argument {} has type `{}`, which is not assignable to parameter `{}` of type `{}`",
                        index + 1,
                        type_label(&actual),
                        parameter.name,
                        type_label(&expected)
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    fn select_function_signature<'b>(
        &self,
        signatures: &'b [FunctionSignature],
        actuals: &[Type],
        explicit_type_arguments: Option<&[Type]>,
    ) -> Result<Option<&'b FunctionSignature>, ()> {
        for signature in signatures {
            if function_signature_matches(
                signature,
                actuals,
                explicit_type_arguments,
                &self.types,
                self.max_type_expansions,
            )? {
                return Ok(Some(signature));
            }
        }
        Ok(None)
    }

    fn is_assignable_bounded(&mut self, actual: &Type, expected: &Type, span: &SourceSpan) -> bool {
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        let assignable = is_assignable(
            actual,
            expected,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        );
        if budget.exhausted {
            self.type_error(
                span,
                format!(
                    "type comparison exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            );
            true
        } else {
            assignable
        }
    }

    fn check_call_type_parameter_constraints(
        &mut self,
        signature: &FunctionSignature,
        substitutions: &BTreeMap<String, Type>,
        span: &SourceSpan,
        actual_description: &str,
    ) {
        for parameter in &signature.type_parameters {
            let Some(constraint) = &parameter.constraint else {
                continue;
            };
            let actual = substitutions
                .get(&parameter.name)
                .expect("function substitutions contain every type parameter");
            let expected = substitute_type(constraint, substitutions);
            if !self.is_assignable_bounded(actual, &expected, span) {
                self.type_error(
                    span,
                    format!(
                        "{actual_description} `{}` does not satisfy constraint `{}` for `{}`",
                        type_label(actual),
                        type_label(&expected),
                        parameter.name,
                    ),
                    DiagnosticCode::TypeMismatch,
                );
            }
        }
    }

    fn check_explicit_function_type_arguments(
        &mut self,
        signature: &FunctionSignature,
        arguments: &[Type],
        span: &SourceSpan,
    ) -> Option<BTreeMap<String, Type>> {
        let required = signature
            .type_parameters
            .iter()
            .filter(|parameter| parameter.default.is_none())
            .count();
        if arguments.len() < required || arguments.len() > signature.type_parameters.len() {
            self.type_error(
                span,
                format!(
                    "function type arguments require {required} to {} argument(s), got {}",
                    signature.type_parameters.len(),
                    arguments.len(),
                ),
                DiagnosticCode::TypeMismatch,
            );
            return None;
        }
        for argument in arguments {
            self.check_type(argument, span);
        }
        let completed = complete_type_arguments(&signature.type_parameters, arguments)?;
        let substitutions = type_parameter_substitutions(&signature.type_parameters, completed);
        self.check_call_type_parameter_constraints(
            signature,
            &substitutions,
            span,
            "type argument",
        );
        Some(substitutions)
    }

    fn check_direct_property_access(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let [base, dot, property] = tokens else {
            return;
        };
        if base.kind != TokenKind::Identifier
            || !dot.is(".")
            || property.kind != TokenKind::Identifier
        {
            return;
        }
        let value = scope.get(&base.text).cloned().unwrap_or(Type::Unknown);
        let mut budget = TypeExpansionBudget::new(self.max_type_expansions);
        match property_type(
            &value,
            &property.text,
            &self.types,
            &mut HashSet::new(),
            &mut budget,
        ) {
            PropertyType::Found(_) | PropertyType::Indeterminate => {}
            PropertyType::Missing => self.type_error(
                span,
                format!(
                    "property `{}` does not exist on type `{}`",
                    property.text,
                    type_label(&value)
                ),
                DiagnosticCode::TypeMismatch,
            ),
            PropertyType::Exhausted => self.type_error(
                span,
                format!(
                    "property lookup exceeds the {} generic-expansion limit",
                    self.max_type_expansions
                ),
                DiagnosticCode::ResourceLimit,
            ),
        }
    }

    /// Checks only arithmetic forms whose operand types are already known
    /// primitive values. Unknown, `any`, union and structural forms remain
    /// outside this deliberately bounded compatibility rule.
    fn check_arithmetic_operators(
        &mut self,
        tokens: &[Token],
        scope: &BTreeMap<String, Type>,
        span: &SourceSpan,
    ) {
        let tokens = strip_outer_parentheses(tokens);
        let generic_call = |start| self.module.generic_call_type_arguments.contains_key(&start);
        if let Some((condition, consequent, alternate)) = conditional_expression_parts(tokens) {
            self.check_arithmetic_operators(condition, scope, span);
            self.check_arithmetic_operators(consequent, scope, span);
            self.check_arithmetic_operators(alternate, scope, span);
            return;
        }
        for operators in [
            &["||"][..],
            &["&&"][..],
            &["===", "!==", "==", "!=", "<", ">", "<=", ">="][..],
        ] {
            if let Some((left, _, right)) = top_level_binary_parts(tokens, operators, generic_call)
            {
                self.check_arithmetic_operators(left, scope, span);
                self.check_arithmetic_operators(right, scope, span);
                return;
            }
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["+", "-"], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_arithmetic_operands(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
            return;
        }
        if let Some((left, operator, right)) =
            top_level_binary_parts(tokens, &["*", "/", "%"], generic_call)
        {
            self.check_arithmetic_operators(left, scope, span);
            self.check_arithmetic_operators(right, scope, span);
            self.check_known_arithmetic_operands(
                operator,
                &self.infer_expression(left, scope),
                &self.infer_expression(right, scope),
                span,
            );
        }
    }

    fn check_known_arithmetic_operands(
        &mut self,
        operator: &Token,
        left: &Type,
        right: &Type,
        span: &SourceSpan,
    ) {
        if !is_known_primitive_type(left) || !is_known_primitive_type(right) {
            return;
        }
        let accepted = (operator.is("+") && (left == &Type::String || right == &Type::String))
            || (left == &Type::Number && right == &Type::Number);
        if !accepted {
            self.type_error(
                span,
                format!(
                    "operator `{}` cannot be applied to types `{}` and `{}`",
                    operator.text,
                    type_label(left),
                    type_label(right),
                ),
                DiagnosticCode::TypeMismatch,
            );
        }
    }
}

fn interface_value(interface: &InterfaceDeclaration) -> Type {
    interface_value_with_heritage(&interface.heritage, &interface.fields)
}

fn interface_value_with_heritage(heritage: &[Type], fields: &[TypeField]) -> Type {
    if heritage.is_empty() {
        return Type::Record(fields.to_vec());
    }
    let mut parts = heritage.to_vec();
    if !fields.is_empty() {
        parts.push(Type::Record(fields.to_vec()));
    }
    if parts.len() == 1 {
        parts.pop().expect("one inherited interface type")
    } else {
        Type::Intersection(parts)
    }
}

fn infer_call_substitutions(
    signature: &FunctionSignature,
    actuals: &[Type],
) -> BTreeMap<String, Type> {
    let mut substitutions = BTreeMap::new();
    let type_parameters = signature
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect::<BTreeSet<_>>();
    for (parameter, actual) in signature.parameters.iter().zip(actuals) {
        let Some(annotation) = &parameter.annotation else {
            continue;
        };
        infer_type_arguments(annotation, actual, &type_parameters, &mut substitutions);
    }
    for parameter in &signature.type_parameters {
        let default = parameter
            .default
            .as_ref()
            .map(|value| substitute_type(value, &substitutions))
            .unwrap_or(Type::Unknown);
        substitutions
            .entry(parameter.name.clone())
            .or_insert(default);
    }
    substitutions
}

fn function_call_substitutions(
    signature: &FunctionSignature,
    actuals: &[Type],
    explicit_type_arguments: Option<&[Type]>,
) -> Option<BTreeMap<String, Type>> {
    match explicit_type_arguments {
        Some(arguments) => complete_type_arguments(&signature.type_parameters, arguments)
            .map(|arguments| type_parameter_substitutions(&signature.type_parameters, arguments)),
        None => Some(infer_call_substitutions(signature, actuals)),
    }
}

fn function_signature_matches(
    signature: &FunctionSignature,
    actuals: &[Type],
    explicit_type_arguments: Option<&[Type]>,
    aliases: &BTreeMap<String, TypeDefinition>,
    max_type_expansions: usize,
) -> Result<bool, ()> {
    let required = signature
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    if actuals.len() < required || actuals.len() > signature.parameters.len() {
        return Ok(false);
    }
    let Some(substitutions) =
        function_call_substitutions(signature, actuals, explicit_type_arguments)
    else {
        return Ok(false);
    };
    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    for parameter in &signature.type_parameters {
        let Some(constraint) = &parameter.constraint else {
            continue;
        };
        let actual = substitutions
            .get(&parameter.name)
            .expect("function substitutions contain every type parameter");
        let expected = substitute_type(constraint, &substitutions);
        if !is_assignable(actual, &expected, aliases, &mut HashSet::new(), &mut budget) {
            return if budget.exhausted { Err(()) } else { Ok(false) };
        }
        if budget.exhausted {
            return Err(());
        }
    }
    for (parameter, actual) in signature.parameters.iter().zip(actuals) {
        let expected = parameter_expected_type(parameter, &substitutions);
        if !is_assignable(actual, &expected, aliases, &mut HashSet::new(), &mut budget) {
            return if budget.exhausted { Err(()) } else { Ok(false) };
        }
        if budget.exhausted {
            return Err(());
        }
    }
    Ok(true)
}

fn overload_is_compatible_with_implementation(
    overload: &FunctionDeclaration,
    implementation: &FunctionDeclaration,
    aliases: &BTreeMap<String, TypeDefinition>,
    max_type_expansions: usize,
) -> Result<bool, ()> {
    let overload_required = overload
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    let implementation_required = implementation
        .parameters
        .iter()
        .filter(|parameter| !parameter.optional)
        .count();
    if overload_required < implementation_required
        || overload.parameters.len() > implementation.parameters.len()
    {
        return Ok(false);
    }
    let overload_substitutions = type_parameter_constraint_substitutions(&overload.type_parameters);
    let implementation_substitutions =
        type_parameter_constraint_substitutions(&implementation.type_parameters);
    let mut budget = TypeExpansionBudget::new(max_type_expansions);
    for (overload_parameter, implementation_parameter) in
        overload.parameters.iter().zip(&implementation.parameters)
    {
        let actual = parameter_expected_type(overload_parameter, &overload_substitutions);
        let expected =
            parameter_expected_type(implementation_parameter, &implementation_substitutions);
        if !is_assignable(
            &actual,
            &expected,
            aliases,
            &mut HashSet::new(),
            &mut budget,
        ) {
            return if budget.exhausted { Err(()) } else { Ok(false) };
        }
        if budget.exhausted {
            return Err(());
        }
    }
    let actual = overload
        .return_type
        .as_ref()
        .map(|value| substitute_type(value, &overload_substitutions))
        .unwrap_or(Type::Unknown);
    let expected = implementation
        .return_type
        .as_ref()
        .map(|value| substitute_type(value, &implementation_substitutions))
        .unwrap_or(Type::Unknown);
    let compatible = is_assignable(
        &actual,
        &expected,
        aliases,
        &mut HashSet::new(),
        &mut budget,
    );
    if budget.exhausted {
        Err(())
    } else {
        Ok(compatible)
    }
}

fn parameter_expected_type(parameter: &Parameter, substitutions: &BTreeMap<String, Type>) -> Type {
    let value = parameter
        .annotation
        .as_ref()
        .map(|annotation| substitute_type(annotation, substitutions))
        .unwrap_or(Type::Unknown);
    if parameter.optional {
        Type::Union(vec![value, Type::Undefined])
    } else {
        value
    }
}

fn type_parameter_constraint_substitutions(parameters: &[TypeParameter]) -> BTreeMap<String, Type> {
    parameters
        .iter()
        .map(|parameter| {
            (
                parameter.name.clone(),
                parameter.constraint.clone().unwrap_or(Type::Unknown),
            )
        })
        .collect()
}

fn type_parameter_substitutions(
    parameters: &[TypeParameter],
    arguments: Vec<Type>,
) -> BTreeMap<String, Type> {
    parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .zip(arguments)
        .collect()
}

struct DirectCall<'a> {
    callee: &'a Token,
    arguments: &'a [Token],
    generic: bool,
}

fn direct_call_parts(tokens: &[Token]) -> Option<DirectCall<'_>> {
    let callee = tokens.first()?;
    if callee.kind != TokenKind::Identifier {
        return None;
    }
    if tokens.get(1).is_some_and(|token| token.is("(")) {
        return Some(DirectCall {
            callee,
            arguments: &tokens[2..],
            generic: false,
        });
    }
    if !tokens.get(1).is_some_and(|token| token.is("<")) {
        return None;
    }
    let close = matching_call_angle_bracket(tokens, 1)?;
    if !tokens.get(close + 1).is_some_and(|token| token.is("(")) {
        return None;
    }
    Some(DirectCall {
        callee,
        arguments: &tokens[close + 2..],
        generic: true,
    })
}

fn matching_call_angle_bracket(tokens: &[Token], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "<" => depth += 1,
            ">" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_call_arguments(tokens: &[Token]) -> Option<Vec<&[Token]>> {
    let close = tokens.iter().position(|token| token.is(")"))?;
    if close + 1 != tokens.len() {
        return None;
    }
    if close == 0 {
        return Some(Vec::new());
    }
    let mut arguments = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    for (index, token) in tokens[..close].iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            "," if depth == 0 => {
                arguments.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    arguments.push(&tokens[start..close]);
    Some(arguments)
}

fn infer_type_arguments(
    template: &Type,
    actual: &Type,
    type_parameters: &BTreeSet<String>,
    substitutions: &mut BTreeMap<String, Type>,
) {
    match (template, actual) {
        (Type::Named { name, arguments }, actual)
            if arguments.is_empty() && type_parameters.contains(name) =>
        {
            if let Some(previous) = substitutions.get(name) {
                if previous != actual {
                    substitutions.insert(name.clone(), Type::Unknown);
                }
            } else {
                substitutions.insert(name.clone(), actual.clone());
            }
        }
        (Type::Array(template), Type::Array(actual)) => {
            infer_type_arguments(template, actual, type_parameters, substitutions);
        }
        (Type::Tuple(templates), Type::Tuple(actuals)) if templates.len() == actuals.len() => {
            for (template, actual) in templates.iter().zip(actuals) {
                infer_type_arguments(template, actual, type_parameters, substitutions);
            }
        }
        (Type::Record(templates), Type::Record(actuals)) => {
            for template in templates {
                if let Some(actual) = actuals.iter().find(|actual| actual.name == template.name) {
                    infer_type_arguments(
                        &template.value,
                        &actual.value,
                        type_parameters,
                        substitutions,
                    );
                }
            }
        }
        _ => {}
    }
}

enum PropertyType {
    Found(Type),
    Missing,
    /// The initial checker has no property semantics for this expression, so
    /// retain its conservative `unknown` behavior rather than rejecting a
    /// potentially valid JavaScript property access.
    Indeterminate,
    Exhausted,
}

fn property_type(
    value: &Type,
    property: &str,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> PropertyType {
    match value {
        Type::Record(fields) => fields
            .iter()
            .find(|field| field.name == property)
            .map(|field| {
                if field.optional {
                    PropertyType::Found(Type::Union(vec![field.value.clone(), Type::Undefined]))
                } else {
                    PropertyType::Found(field.value.clone())
                }
            })
            .unwrap_or(PropertyType::Missing),
        Type::Named { .. } => {
            match instantiate_named(value, aliases, visited, budget, "property") {
                Some(value) => property_type(&value, property, aliases, visited, budget),
                None if budget.exhausted => PropertyType::Exhausted,
                None => PropertyType::Indeterminate,
            }
        }
        Type::Intersection(parts) => {
            let mut indeterminate = false;
            for part in parts {
                match property_type(part, property, aliases, visited, budget) {
                    PropertyType::Found(value) => return PropertyType::Found(value),
                    PropertyType::Missing => {}
                    PropertyType::Indeterminate => indeterminate = true,
                    PropertyType::Exhausted => return PropertyType::Exhausted,
                }
            }
            if indeterminate {
                PropertyType::Indeterminate
            } else {
                PropertyType::Missing
            }
        }
        Type::Any | Type::Unknown => PropertyType::Indeterminate,
        _ => PropertyType::Missing,
    }
}

fn infer_array(tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
    let mut values = Vec::new();
    let mut start = 1usize;
    let mut depth = 0usize;
    for index in 1..tokens.len() {
        match tokens[index].text.as_str() {
            "[" | "(" | "{" => depth += 1,
            "]" | ")" | "}" if depth > 0 => depth -= 1,
            "," if depth == 0 => {
                if start < index {
                    values.push(infer_simple(&tokens[start..index], scope));
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if start + 1 < tokens.len() {
        values.push(infer_simple(&tokens[start..tokens.len() - 1], scope));
    }
    let Some(first) = values.first().cloned() else {
        return Type::Array(Box::new(Type::Unknown));
    };
    if values.iter().all(|value| value == &first) {
        Type::Array(Box::new(first))
    } else {
        Type::Array(Box::new(Type::Union(values)))
    }
}

fn infer_record(tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
    let mut fields = Vec::new();
    let mut index = 1usize;
    while index + 2 < tokens.len() && !tokens[index].is("}") {
        let name = tokens[index].text.clone();
        if !tokens[index + 1].is(":") {
            return Type::Unknown;
        }
        let value_start = index + 2;
        let mut value_end = value_start;
        while value_end < tokens.len() && !tokens[value_end].is(",") && !tokens[value_end].is("}") {
            value_end += 1;
        }
        fields.push(TypeField {
            name,
            optional: false,
            value: infer_simple(&tokens[value_start..value_end], scope),
            span: SourceSpan::new(
                "<inferred>",
                tokens[index].start,
                tokens[value_end.saturating_sub(1)].end,
            ),
        });
        index = value_end.saturating_add(1);
    }
    Type::Record(fields)
}

fn infer_simple(tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
    let Some(first) = tokens.first() else {
        return Type::Undefined;
    };
    if first.kind == TokenKind::String || first.kind == TokenKind::Template {
        Type::String
    } else if first.kind == TokenKind::Number {
        Type::Number
    } else if matches!(first.text.as_str(), "true" | "false") {
        Type::Boolean
    } else if first.text == "null" {
        Type::Null
    } else if first.kind == TokenKind::Identifier {
        scope.get(&first.text).cloned().unwrap_or(Type::Unknown)
    } else {
        Type::Unknown
    }
}

/// Removes matching parentheses that wrap an entire expression. This does not
/// parse JavaScript generally; it only exposes a nested expression to the
/// bounded inference rules below.
fn strip_outer_parentheses(mut tokens: &[Token]) -> &[Token] {
    while tokens.len() >= 2 && tokens.first().is_some_and(|token| token.is("(")) {
        let mut depth = 0usize;
        let mut closes_at_end = false;
        for (index, token) in tokens.iter().enumerate() {
            match token.text.as_str() {
                "(" => depth += 1,
                ")" if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        closes_at_end = index + 1 == tokens.len();
                        break;
                    }
                }
                _ => {}
            }
        }
        if !closes_at_end {
            break;
        }
        tokens = &tokens[1..tokens.len() - 1];
    }
    tokens
}

/// Returns the operands and final top-level operator from `operators`.
/// Selecting the final occurrence preserves left associativity for the
/// bounded expression operators this checker supports.
fn top_level_binary_parts<'a>(
    tokens: &'a [Token],
    operators: &[&str],
    is_explicit_generic_call: impl Fn(usize) -> bool,
) -> Option<(&'a [Token], &'a Token, &'a [Token])> {
    let mut depth = 0usize;
    let mut operator_index = None;
    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.kind == TokenKind::Identifier && is_explicit_generic_call(token.start) {
            if let Some(close) = explicit_generic_call_close(tokens, index) {
                index = close + 1;
                continue;
            }
        }
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            _ if depth == 0
                && operators.contains(&token.text.as_str())
                && !is_prefix_arithmetic_operator(tokens, index) =>
            {
                operator_index = Some(index);
            }
            _ => {}
        }
        index += 1;
    }
    let index = operator_index?;
    (index > 0 && index + 1 < tokens.len()).then_some((
        &tokens[..index],
        &tokens[index],
        &tokens[index + 1..],
    ))
}

fn is_prefix_arithmetic_operator(tokens: &[Token], index: usize) -> bool {
    if !tokens
        .get(index)
        .is_some_and(|token| matches!(token.text.as_str(), "+" | "-"))
    {
        return false;
    }
    index == 0
        || tokens.get(index - 1).is_some_and(|previous| {
            matches!(
                previous.text.as_str(),
                "(" | "[" | "{" | "?" | ":" | "," | "=" | "+" | "-" | "*" | "/" | "%"
            )
        })
}

/// Finds the closing angle bracket for a parser-confirmed explicit generic
/// call. The parser's source-offset table disambiguates these brackets from
/// ordinary relational operators before this bounded expression scan runs.
fn explicit_generic_call_close(tokens: &[Token], callee_index: usize) -> Option<usize> {
    if !tokens
        .get(callee_index + 1)
        .is_some_and(|token| token.is("<"))
    {
        return None;
    }
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(callee_index + 1) {
        match token.text.as_str() {
            "<" => depth += 1,
            ">" if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return tokens
                        .get(index + 1)
                        .is_some_and(|token| token.is("("))
                        .then_some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits one top-level conditional expression into its condition and branch
/// expressions. Nested conditionals are accounted for before accepting their
/// matching colon, so `a ? b : c ? d : e` remains well formed.
fn conditional_expression_parts(tokens: &[Token]) -> Option<(&[Token], &[Token], &[Token])> {
    let mut depth = 0usize;
    let mut question_index = None;
    let mut nested_conditionals = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" if depth > 0 => depth -= 1,
            "?" if depth == 0 => {
                if question_index.is_none() {
                    question_index = Some(index);
                } else {
                    nested_conditionals += 1;
                }
            }
            ":" if depth == 0 && question_index.is_some() => {
                if nested_conditionals == 0 {
                    let question_index = question_index.expect("conditional question index exists");
                    return (question_index > 0
                        && index > question_index + 1
                        && index + 1 < tokens.len())
                    .then_some((
                        &tokens[..question_index],
                        &tokens[question_index + 1..index],
                        &tokens[index + 1..],
                    ));
                }
                nested_conditionals -= 1;
            }
            _ => {}
        }
    }
    None
}

fn infer_boolean_logical_expression(left: Type, right: Type) -> Type {
    if left == Type::Boolean && right == Type::Boolean {
        Type::Boolean
    } else {
        Type::Unknown
    }
}

fn infer_additive_expression(operator: &Token, left: Type, right: Type) -> Type {
    if operator.is("+") && (left == Type::String || right == Type::String) {
        Type::String
    } else {
        infer_numeric_binary_expression(left, right)
    }
}

fn infer_numeric_binary_expression(left: Type, right: Type) -> Type {
    if left == Type::Number && right == Type::Number {
        Type::Number
    } else {
        Type::Unknown
    }
}

fn is_known_primitive_type(value: &Type) -> bool {
    matches!(
        value,
        Type::Boolean | Type::Number | Type::String | Type::Null | Type::Undefined
    )
}

fn merge_conditional_branch_types(consequent: Type, alternate: Type) -> Type {
    if consequent == alternate {
        consequent
    } else {
        Type::Union(vec![consequent, alternate])
    }
}

fn is_assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
) -> bool {
    if matches!(actual, Type::Any | Type::Unknown) || matches!(expected, Type::Any | Type::Unknown)
    {
        return true;
    }
    if actual == expected {
        return true;
    }
    if let Some(expanded) = instantiate_named(actual, aliases, visited, budget, "actual") {
        return is_assignable(&expanded, expected, aliases, visited, budget);
    }
    if let Some(expanded) = instantiate_named(expected, aliases, visited, budget, "expected") {
        return is_assignable(actual, &expanded, aliases, visited, budget);
    }
    if let Type::Union(options) = actual {
        return options
            .iter()
            .all(|option| is_assignable(option, expected, aliases, &mut visited.clone(), budget));
    }
    if let Type::Union(options) = expected {
        return options
            .iter()
            .any(|option| is_assignable(actual, option, aliases, &mut visited.clone(), budget));
    }
    if let Type::Intersection(parts) = expected {
        return parts
            .iter()
            .all(|part| is_assignable(actual, part, aliases, &mut visited.clone(), budget));
    }
    if let (Type::Intersection(_), Type::Record(expected_fields)) = (actual, expected) {
        return expected_fields.iter().all(|expected_field| {
            match property_type(actual, &expected_field.name, aliases, visited, budget) {
                PropertyType::Found(actual) => is_assignable(
                    &actual,
                    &expected_field.value,
                    aliases,
                    &mut visited.clone(),
                    budget,
                ),
                PropertyType::Missing => expected_field.optional,
                PropertyType::Indeterminate => true,
                PropertyType::Exhausted => false,
            }
        });
    }
    if let Type::Intersection(parts) = actual {
        return parts
            .iter()
            .any(|part| is_assignable(part, expected, aliases, &mut visited.clone(), budget));
    }
    match (actual, expected) {
        (Type::Literal(value), Type::String) => value.starts_with('\'') || value.starts_with('\"'),
        (Type::Literal(value), Type::Number) => value.parse::<f64>().is_ok(),
        (Type::Literal(value), Type::Boolean) => matches!(value.as_str(), "true" | "false"),
        (Type::Array(actual), Type::Array(expected)) => {
            is_assignable(actual, expected, aliases, visited, budget)
        }
        (Type::Tuple(actual), Type::Tuple(expected)) if actual.len() == expected.len() => {
            actual.iter().zip(expected).all(|(actual, expected)| {
                is_assignable(actual, expected, aliases, &mut visited.clone(), budget)
            })
        }
        (Type::Record(actual), Type::Record(expected)) => expected.iter().all(|expected_field| {
            actual
                .iter()
                .find(|actual_field| actual_field.name == expected_field.name)
                .map(|actual_field| {
                    (actual_field.optional == expected_field.optional || expected_field.optional)
                        && is_assignable(
                            &actual_field.value,
                            &expected_field.value,
                            aliases,
                            &mut visited.clone(),
                            budget,
                        )
                })
                .unwrap_or(expected_field.optional)
        }),
        _ => actual == expected,
    }
}

fn instantiate_named(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
    visited: &mut HashSet<String>,
    budget: &mut TypeExpansionBudget,
    side: &str,
) -> Option<Type> {
    let Type::Named { name, arguments } = value else {
        return None;
    };
    let definition = aliases.get(name)?;
    let arguments = complete_type_arguments(&definition.parameters, arguments)?;
    let key = format!("{side}:{}", type_identity(value));
    if !visited.insert(key) {
        return None;
    }
    if !budget.consume() {
        return None;
    }
    let substitutions = definition
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .zip(arguments)
        .collect();
    Some(substitute_type(&definition.value, &substitutions))
}

/// Resolves omitted trailing generic arguments through their declaration-site
/// defaults. The parser/checker reports invalid argument counts and constraint
/// violations separately; this helper is also used by structural expansion,
/// where `None` simply means the named type cannot be expanded safely.
fn complete_type_arguments(parameters: &[TypeParameter], supplied: &[Type]) -> Option<Vec<Type>> {
    if supplied.len() > parameters.len() {
        return None;
    }
    let mut substitutions = BTreeMap::new();
    let mut arguments = Vec::with_capacity(parameters.len());
    for (index, parameter) in parameters.iter().enumerate() {
        let value = supplied.get(index).cloned().or_else(|| {
            parameter
                .default
                .as_ref()
                .map(|value| substitute_type(value, &substitutions))
        })?;
        substitutions.insert(parameter.name.clone(), value.clone());
        arguments.push(value);
    }
    Some(arguments)
}

fn substitute_type(value: &Type, substitutions: &BTreeMap<String, Type>) -> Type {
    match value {
        Type::Named { name, arguments } if arguments.is_empty() => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| value.clone()),
        Type::Named { name, arguments } => Type::Named {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_type(argument, substitutions))
                .collect(),
        },
        Type::Array(value) => Type::Array(Box::new(substitute_type(value, substitutions))),
        Type::Tuple(values) => Type::Tuple(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        Type::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|field| TypeField {
                    name: field.name.clone(),
                    optional: field.optional,
                    value: substitute_type(&field.value, substitutions),
                    span: field.span.clone(),
                })
                .collect(),
        ),
        Type::Union(values) => Type::Union(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        Type::Intersection(values) => Type::Intersection(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn type_identity(value: &Type) -> String {
    match value {
        Type::Named { name, arguments } => format!(
            "{name}<{}>",
            arguments
                .iter()
                .map(type_identity)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Array(value) => format!("{}[]", type_identity(value)),
        Type::Tuple(values) => format!(
            "[{}]",
            values
                .iter()
                .map(type_identity)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Record(_) => "record".to_string(),
        Type::Union(values) => values
            .iter()
            .map(type_identity)
            .collect::<Vec<_>>()
            .join("|"),
        Type::Intersection(values) => values
            .iter()
            .map(type_identity)
            .collect::<Vec<_>>()
            .join("&"),
        _ => type_label(value),
    }
}

pub(crate) fn type_label(value: &Type) -> String {
    match value {
        Type::Any => "any".to_string(),
        Type::Unknown => "unknown".to_string(),
        Type::Never => "never".to_string(),
        Type::Void => "void".to_string(),
        Type::Null => "null".to_string(),
        Type::Undefined => "undefined".to_string(),
        Type::Boolean => "boolean".to_string(),
        Type::Number => "number".to_string(),
        Type::String => "string".to_string(),
        Type::Literal(value) => value.clone(),
        Type::Named { name, .. } => name.clone(),
        Type::Array(value) => format!("{}[]", type_label(value)),
        Type::Tuple(values) => format!(
            "[{}]",
            values.iter().map(type_label).collect::<Vec<_>>().join(", ")
        ),
        Type::Record(_) => "record".to_string(),
        Type::Union(values) => values
            .iter()
            .map(type_label)
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Intersection(values) => values
            .iter()
            .map(type_label)
            .collect::<Vec<_>>()
            .join(" & "),
    }
}

#[cfg(test)]
#[path = "checker/tests.rs"]
mod tests;
