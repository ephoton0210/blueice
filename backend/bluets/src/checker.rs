// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Name binding and deterministic, deliberately bounded type checking.

use crate::compiler::Project;
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{Declaration, FunctionDeclaration, Module, TypeField};
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

pub(crate) fn check(project: &Project, enforce_types: bool) -> (CheckedProject, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let exported_types = exported_types(project);
    let mut checked_modules = BTreeMap::new();

    for (module_id, module) in &project.modules {
        let mut checker = ModuleChecker::new(project, module, &exported_types, enforce_types);
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

fn exported_types(project: &Project) -> BTreeMap<String, BTreeMap<String, Type>> {
    let mut modules = BTreeMap::new();
    for (id, module) in &project.modules {
        let mut values = BTreeMap::new();
        for declaration in &module.declarations {
            match declaration {
                Declaration::TypeAlias(alias) if alias.exported => {
                    values.insert(alias.name.clone(), alias.value.clone());
                }
                Declaration::Interface(interface) if interface.exported => {
                    values.insert(
                        interface.name.clone(),
                        Type::Record(interface.fields.clone()),
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

struct ModuleChecker<'a> {
    project: &'a Project,
    module: &'a Module,
    exported_types: &'a BTreeMap<String, BTreeMap<String, Type>>,
    enforce_types: bool,
    diagnostics: Vec<Diagnostic>,
    symbols: Vec<Symbol>,
    types: BTreeMap<String, Type>,
    values: BTreeMap<String, Type>,
    type_parameters: BTreeSet<String>,
}

impl<'a> ModuleChecker<'a> {
    fn new(
        project: &'a Project,
        module: &'a Module,
        exported_types: &'a BTreeMap<String, BTreeMap<String, Type>>,
        enforce_types: bool,
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
            type_parameters: BTreeSet::new(),
        }
    }

    fn bind(&mut self) {
        for declaration in &self.module.declarations {
            match declaration {
                Declaration::Import(import) => self.bind_import(import),
                Declaration::TypeExport(export) => self.bind_type_export(export),
                Declaration::TypeAlias(alias) => {
                    self.type_parameters
                        .extend(alias.type_parameters.iter().cloned());
                    self.insert_type(
                        &alias.name,
                        alias.value.clone(),
                        alias.span.clone(),
                        SymbolKind::TypeAlias,
                        alias.exported,
                    );
                }
                Declaration::Interface(interface) => {
                    self.type_parameters
                        .extend(interface.type_parameters.iter().cloned());
                    self.insert_type(
                        &interface.name,
                        Type::Record(interface.fields.clone()),
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
                    self.insert_value(
                        &function.name,
                        value_type,
                        function.span.clone(),
                        SymbolKind::Function,
                        function.exported,
                    );
                }
                Declaration::Raw(_) => {}
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
        value_type: Type,
        span: SourceSpan,
        kind: SymbolKind,
        exported: bool,
    ) {
        if self
            .types
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
                Declaration::TypeAlias(alias) => self.check_type(&alias.value, &alias.span),
                Declaration::Interface(interface) => {
                    for field in &interface.fields {
                        self.check_type(&field.value, &field.span);
                    }
                }
                Declaration::Variable(variable) => self.check_variable(variable),
                Declaration::Function(function) => self.check_function(function),
                Declaration::Import(_) | Declaration::TypeExport(_) | Declaration::Raw(_) => {}
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
        let inferred = self.infer_expression(&variable.initializer, scope);
        if !is_assignable(&inferred, annotation, &self.types, &mut HashSet::new()) {
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
        self.type_parameters
            .extend(function.type_parameters.iter().cloned());
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
                let actual = self.infer_expression(returned, &scope);
                if !is_assignable(&actual, return_type, &self.types, &mut HashSet::new()) {
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
                if !self.types.contains_key(name) && !self.type_parameters.contains(name) {
                    self.type_error(
                        span,
                        format!("cannot find type `{name}`"),
                        DiagnosticCode::UnknownType,
                    );
                }
                for argument in arguments {
                    self.check_type(argument, span);
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

    fn type_error(&mut self, span: &SourceSpan, message: String, code: DiagnosticCode) {
        if self.enforce_types {
            self.diagnostics
                .push(Diagnostic::error(code, span.clone(), message));
        }
    }

    fn infer_expression(&self, tokens: &[Token], scope: &BTreeMap<String, Type>) -> Type {
        let Some(first) = tokens.first() else {
            return Type::Undefined;
        };
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
                if tokens.get(1).is_some_and(|token| token.is("(")) {
                    return scope.get(&first.text).cloned().unwrap_or(Type::Unknown);
                }
                scope.get(&first.text).cloned().unwrap_or(Type::Unknown)
            }
            _ => Type::Unknown,
        }
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

fn is_assignable(
    actual: &Type,
    expected: &Type,
    aliases: &BTreeMap<String, Type>,
    visited: &mut HashSet<String>,
) -> bool {
    if matches!(actual, Type::Any | Type::Unknown) || matches!(expected, Type::Any | Type::Unknown)
    {
        return true;
    }
    if let Type::Named { name, .. } = expected {
        if actual == expected {
            return true;
        }
        if !visited.insert(name.clone()) {
            return true;
        }
        return aliases
            .get(name)
            .is_some_and(|alias| is_assignable(actual, alias, aliases, visited));
    }
    if let Type::Union(options) = expected {
        return options
            .iter()
            .any(|option| is_assignable(actual, option, aliases, &mut visited.clone()));
    }
    if let Type::Intersection(parts) = expected {
        return parts
            .iter()
            .all(|part| is_assignable(actual, part, aliases, &mut visited.clone()));
    }
    match (actual, expected) {
        (Type::Literal(value), Type::String) => value.starts_with('\'') || value.starts_with('\"'),
        (Type::Literal(value), Type::Number) => value.parse::<f64>().is_ok(),
        (Type::Literal(value), Type::Boolean) => matches!(value.as_str(), "true" | "false"),
        (Type::Array(actual), Type::Array(expected)) => {
            is_assignable(actual, expected, aliases, visited)
        }
        (Type::Tuple(actual), Type::Tuple(expected)) if actual.len() == expected.len() => {
            actual.iter().zip(expected).all(|(actual, expected)| {
                is_assignable(actual, expected, aliases, &mut visited.clone())
            })
        }
        (Type::Record(actual), Type::Record(expected)) => expected.iter().all(|expected_field| {
            actual
                .iter()
                .find(|actual_field| actual_field.name == expected_field.name)
                .map(|actual_field| {
                    is_assignable(
                        &actual_field.value,
                        &expected_field.value,
                        aliases,
                        &mut visited.clone(),
                    )
                })
                .unwrap_or(expected_field.optional)
        }),
        _ => actual == expected,
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
mod tests {
    use super::*;
    use crate::compiler::{CompilerOptions, MapLoader, ModuleSource};

    #[test]
    fn rejects_a_primitive_initializer_with_the_wrong_annotation() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///app.ts",
            "const count: number = 'one';",
        )]);
        let result = crate::compile("memory:///app.ts", &loader, CompilerOptions::default());
        assert!(result.has_errors());
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));
    }

    #[test]
    fn accepts_a_structurally_compatible_record() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///app.ts",
            "interface User { name: string; age?: number } const user: User = { name: 'Ada' };",
        )]);
        let result = crate::compile("memory:///app.ts", &loader, CompilerOptions::default());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
    }

    #[test]
    fn checks_and_erases_a_local_typed_variable_before_its_return() {
        let loader = MapLoader::from([ModuleSource::new(
            "memory:///app.ts",
            "function count(): number { const local: number = 'wrong'; return local; }",
        )]);
        let result = crate::compile("memory:///app.ts", &loader, CompilerOptions::default());
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));
        assert!(result.output.is_none());
    }

    #[test]
    fn resolves_a_type_through_a_type_only_reexport() {
        let loader = MapLoader::from([
            ModuleSource::new(
                "memory:///main.ts",
                "import type { PublicUser } from './api.ts'; const user: PublicUser = { id: 'ada' };",
            ),
            ModuleSource::new(
                "memory:///api.ts",
                "export type { User as PublicUser } from './model.ts';",
            ),
            ModuleSource::new(
                "memory:///model.ts",
                "export interface User { id: string }",
            ),
        ]);
        let result = crate::compile("memory:///main.ts", &loader, CompilerOptions::default());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
    }
}
