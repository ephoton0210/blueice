// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Names and import retention for declarations inferred from checked values.

use super::*;
use crate::checker::CheckedModule;
use crate::parser::{ImportDeclaration, VariableDeclaration, VariableKind};
use std::collections::BTreeSet;

pub(super) struct Context<'a> {
    module: &'a Module,
    project: &'a Project,
    used_imports: BTreeSet<String>,
    variables: BTreeMap<String, (String, bool)>,
}

impl<'a> Context<'a> {
    pub(super) fn new(
        checked: &'a CheckedModule,
        project: &'a Project,
    ) -> Result<Self, Diagnostic> {
        let module = &checked.module;
        let mut context = Self {
            module,
            project,
            used_imports: BTreeSet::new(),
            variables: BTreeMap::new(),
        };
        for declaration in &module.declarations {
            let Declaration::Variable(variable) = declaration else {
                continue;
            };
            if variable.declared
                || !(variable.exported
                    || is_default_export_name(module, &variable.name)
                    || is_value_export_name(module, &variable.name))
            {
                continue;
            }
            if let Some(annotation) = &variable.annotation {
                context.references(annotation);
                continue;
            }
            let value = checked
                .symbols
                .iter()
                .find(|symbol| {
                    symbol.kind == crate::checker::SymbolKind::Variable
                        && symbol.name == variable.name
                        && symbol.span == variable.span
                })
                .and_then(|symbol| symbol.value_type.as_ref())
                .unwrap_or(&Type::Unknown);
            let value = if let Some(name) = context.function_initializer(variable) {
                context.retain(&name);
                format!("typeof {name}")
            } else {
                context.render(value, 0, &variable.span)?
            };
            let literal = matches!(
                checked
                    .symbols
                    .iter()
                    .find(|symbol| symbol.name == variable.name)
                    .and_then(|symbol| symbol.value_type.as_ref()),
                Some(Type::Literal(_))
            );
            let primitive = matches!(checked.symbols.iter().find(|symbol| symbol.name == variable.name)
                .and_then(|symbol| symbol.value_type.as_ref()), Some(Type::Literal(text))
                    if text.starts_with(['\'', '"', '`']) || text.parse::<f64>().is_ok() || matches!(text.as_str(), "true" | "false"));
            let initializer = variable.kind == VariableKind::Const
                && literal
                && (!primitive
                    || fresh_variable(module, variable, project, &mut BTreeSet::new(), 0));
            context
                .variables
                .insert(variable.name.clone(), (value, initializer));
        }
        // Value imports also appear in annotations, class heritage and signatures.
        for declaration in &module.declarations {
            match declaration {
                Declaration::Function(function)
                    if function.exported
                        || is_default_export_name(module, &function.name)
                        || is_value_export_name(module, &function.name) =>
                {
                    for parameter in &function.parameters {
                        if let Some(value) = &parameter.annotation {
                            context.references(value);
                        }
                    }
                    if let Some(value) = &function.return_type {
                        context.references(value);
                    }
                }
                Declaration::Class(class)
                    if class.exported
                        || is_default_export_name(module, &class.name)
                        || is_value_export_name(module, &class.name) =>
                {
                    if let Some(name) = &class.extends_name {
                        context.retain(name);
                    }
                    for member in &class.members {
                        if let Some(field) = &member.field {
                            if let Some(value) = &field.annotation {
                                context.references(value);
                            }
                        }
                        if let Some(method) = &member.method {
                            for parameter in &method.parameters {
                                if let Some(value) = &parameter.annotation {
                                    context.references(value);
                                }
                            }
                            if let Some(value) = &method.return_type {
                                context.references(value);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for symbol in &checked.symbols {
            if symbol.exported {
                if let Some(value) = &symbol.value_type {
                    context.references(value);
                }
            }
        }
        Ok(context)
    }

    pub(super) fn variable(&self, name: &str) -> Option<&(String, bool)> {
        self.variables.get(name)
    }

    fn retain(&mut self, name: &str) {
        let bare = name.strip_prefix("typeof ").unwrap_or(name);
        if !bare.contains('@') {
            self.used_imports
                .insert(bare.split('.').next().unwrap_or(bare).to_string());
        }
    }

    fn references(&mut self, value: &Type) {
        match value {
            Type::Named { name, arguments } => {
                self.retain(name);
                for argument in arguments {
                    self.references(argument);
                }
            }
            Type::Literal(name) if name.contains('.') => self.retain(name),
            Type::Array(element) => self.references(element),
            Type::Tuple(elements) => {
                for element in elements {
                    self.references(&element.annotation);
                }
            }
            Type::Record(fields) => {
                for field in fields {
                    self.references(&field.value);
                }
            }
            Type::Function { parameters, result } => {
                for parameter in parameters {
                    if let Some(value) = &parameter.annotation {
                        self.references(value);
                    }
                }
                self.references(result);
            }
            Type::Union(options) | Type::Intersection(options) => {
                for option in options {
                    self.references(option);
                }
            }
            _ => {}
        }
    }

    fn function_initializer(&self, variable: &VariableDeclaration) -> Option<String> {
        let name = match variable.initializer.as_slice() {
            [name] if name.kind == crate::syntax::TokenKind::Identifier => name.text.clone(),
            [owner, dot, member] if dot.is(".") => format!("{}.{}", owner.text, member.text),
            _ => return None,
        };
        let first = name.split('.').next()?;
        let import = self
            .module
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Import(import) => import
                    .bindings
                    .iter()
                    .find(|binding| binding.local == first)
                    .map(|binding| (import, binding)),
                _ => None,
            })?;
        let resolved = self
            .project
            .resolutions
            .get(&(self.module.id.clone(), import.0.specifier.clone()))?;
        let source = self.project.modules.get(resolved)?;
        let exported = if import.1.imported == "*" {
            name.split_once('.')?.1
        } else {
            &import.1.imported
        };
        let local = exported_local(source, exported);
        declared_function(source, &local, self.project, &mut BTreeSet::new(), 0).then_some(name)
    }

    fn render(
        &mut self,
        value: &Type,
        indent: usize,
        span: &SourceSpan,
    ) -> Result<String, Diagnostic> {
        Ok(match value {
            Type::Named { name, arguments } => {
                let name = if let Some((bare, id)) = name.split_once('@') {
                    let (root, suffix) = bare
                        .split_once('.')
                        .map_or((bare, "".to_string()), |(root, rest)| {
                            (root, format!(".{rest}"))
                        });
                    let source = self.project.modules.get(id);
                    let public = source.and_then(|source| public_type_name(source, root));
                    let specifier =
                        self.module
                            .declarations
                            .iter()
                            .find_map(|declaration| match declaration {
                                Declaration::Import(import)
                                    if self
                                        .project
                                        .resolutions
                                        .get(&(self.module.id.clone(), import.specifier.clone()))
                                        .is_some_and(|source| source == id) =>
                                {
                                    Some(import.specifier.clone())
                                }
                                _ => None,
                            });
                    let Some((public, specifier)) = public.zip(specifier) else {
                        return Err(Diagnostic::error(DiagnosticCode::TypeMismatch, span.clone(), format!("TS4023: exported variable uses external type `{root}` which cannot be named")));
                    };
                    format!("import(\"{specifier}\").{public}{suffix}")
                } else {
                    self.retain(name);
                    crate::parser::source_type_name(name).to_string()
                };
                if arguments.is_empty() {
                    name
                } else {
                    format!(
                        "{name}<{}>",
                        arguments
                            .iter()
                            .map(|value| self.render(value, indent, span))
                            .collect::<Result<Vec<_>, _>>()?
                            .join(", ")
                    )
                }
            }
            Type::Record(fields) => {
                let pad = " ".repeat(indent + 4);
                let mut text = String::from("{\n");
                for field in fields {
                    text.push_str(&format!(
                        "{pad}{}{}{}: {};\n",
                        if field.readonly { "readonly " } else { "" },
                        field.name,
                        if field.optional { "?" } else { "" },
                        self.render(&field.value, indent + 4, span)?
                    ));
                }
                text.push_str(&format!("{}}}", " ".repeat(indent)));
                text
            }
            Type::Array(element) => {
                let text = self.render(element, indent, span)?;
                format!(
                    "{}[]",
                    if matches!(
                        element.as_ref(),
                        Type::Union(_) | Type::Intersection(_) | Type::Function { .. }
                    ) {
                        format!("({text})")
                    } else {
                        text
                    }
                )
            }
            Type::Tuple(elements) => {
                let mut members = Vec::new();
                for element in elements {
                    let annotation = self.render(&element.annotation, indent, span)?;
                    members.push(if let Some(label) = &element.label {
                        format!(
                            "{}{label}{}: {annotation}",
                            if element.rest { "..." } else { "" },
                            if element.optional { "?" } else { "" },
                        )
                    } else {
                        format!(
                            "{}{annotation}{}",
                            if element.rest { "..." } else { "" },
                            if element.optional { "?" } else { "" },
                        )
                    });
                }
                format!("[{}]", members.join(", "))
            }
            Type::Function { parameters, result } => {
                let mut members = Vec::new();
                for parameter in parameters {
                    let annotation = self.render(
                        parameter.annotation.as_ref().unwrap_or(&Type::Unknown),
                        indent,
                        span,
                    )?;
                    members.push(format!(
                        "{}{}{}: {annotation}",
                        if parameter.rest { "..." } else { "" },
                        parameter.name,
                        if parameter.optional { "?" } else { "" },
                    ));
                }
                format!(
                    "({}) => {}",
                    members.join(", "),
                    self.render(result, indent, span)?
                )
            }
            Type::Union(options) | Type::Intersection(options) => options
                .iter()
                .map(|option| self.render(option, indent, span))
                .collect::<Result<Vec<_>, _>>()?
                .join(if matches!(value, Type::Union(_)) {
                    " | "
                } else {
                    " & "
                }),
            Type::Literal(name) => {
                self.retain(name);
                name.clone()
            }
            _ => {
                self.references(value);
                type_to_ts(value)
            }
        })
    }

    pub(super) fn import(&self, import: &ImportDeclaration) -> Option<String> {
        let bindings: Vec<_> = import
            .bindings
            .iter()
            .filter(|binding| self.used_imports.contains(&binding.local))
            .collect();
        if bindings.is_empty() {
            return None;
        }
        let specifier = &self.module.source[import.specifier_span.start..import.specifier_span.end];
        if import.equals_require {
            return Some(format!(
                "import {} = require({specifier});\n",
                bindings[0].local
            ));
        }
        let mut clauses = Vec::new();
        if let Some(binding) = bindings
            .iter()
            .find(|binding| binding.imported == "default")
        {
            clauses.push(binding.local.clone());
        }
        if let Some(binding) = bindings.iter().find(|binding| binding.imported == "*") {
            clauses.push(format!("* as {}", binding.local));
        }
        let named: Vec<_> = bindings
            .iter()
            .filter(|binding| !matches!(binding.imported.as_str(), "default" | "*"))
            .map(|binding| {
                if binding.local == binding.imported {
                    binding.local.clone()
                } else {
                    format!("{} as {}", binding.imported, binding.local)
                }
            })
            .collect();
        if !named.is_empty() {
            clauses.push(format!("{{ {} }}", named.join(", ")));
        }
        Some(format!("import {} from {specifier};\n", clauses.join(", ")))
    }
}

fn public_type_name(module: &Module, name: &str) -> Option<String> {
    for declaration in &module.declarations {
        let direct = match declaration {
            Declaration::Interface(item) => item.exported && item.name == name,
            Declaration::TypeAlias(item) => item.exported && item.name == name,
            Declaration::Class(item) => item.exported && item.name == name,
            Declaration::Enum(item) => item.exported && item.name == name,
            _ => false,
        };
        if direct {
            return Some(name.to_string());
        }
        if let Declaration::TypeExport(export) = declaration {
            if export.specifier.is_none() {
                for binding in &export.bindings {
                    let (local, public) = binding.split_once(" as ").unwrap_or((binding, binding));
                    if local == name {
                        return Some(public.to_string());
                    }
                }
            }
        }
        if let Declaration::ValueExport(export) = declaration {
            if let Some(binding) = export.bindings.iter().find(|binding| binding.local == name) {
                return Some(binding.exported.clone());
            }
        }
    }
    None
}

fn exported_local(module: &Module, exported: &str) -> String {
    for declaration in &module.declarations {
        match declaration {
            Declaration::DefaultExport(item) if exported == "default" => return item.name.clone(),
            Declaration::Function(item) if item.default_export && exported == "default" => {
                return item.name.clone()
            }
            Declaration::ValueExport(item) => {
                if let Some(binding) = item
                    .bindings
                    .iter()
                    .find(|binding| binding.exported == exported)
                {
                    return binding.local.clone();
                }
            }
            _ => {}
        }
    }
    exported.to_string()
}

fn declared_function(
    module: &Module,
    name: &str,
    project: &Project,
    seen: &mut BTreeSet<(String, String)>,
    depth: usize,
) -> bool {
    if depth >= 128 || !seen.insert((module.id.clone(), name.to_string())) {
        return false;
    }
    for declaration in &module.declarations {
        match declaration {
            Declaration::Function(function) if function.name == name => return true,
            Declaration::Variable(variable)
                if variable.name == name && variable.annotation.is_none() =>
            {
                if let [source] = variable.initializer.as_slice() {
                    return declared_function(module, &source.text, project, seen, depth + 1);
                }
            }
            Declaration::Import(import) => {
                if let Some(binding) = import.bindings.iter().find(|binding| binding.local == name)
                {
                    if let Some(source) = project
                        .resolutions
                        .get(&(module.id.clone(), import.specifier.clone()))
                        .and_then(|id| project.modules.get(id))
                    {
                        return declared_function(
                            source,
                            &exported_local(source, &binding.imported),
                            project,
                            seen,
                            depth + 1,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn fresh_variable(
    module: &Module,
    variable: &VariableDeclaration,
    project: &Project,
    seen: &mut BTreeSet<(String, String)>,
    depth: usize,
) -> bool {
    if depth >= 128
        || variable.annotation.is_some()
        || variable.kind != VariableKind::Const
        || !seen.insert((module.id.clone(), variable.name.clone()))
    {
        return false;
    }
    let [value] = variable.initializer.as_slice() else {
        return false;
    };
    if matches!(
        value.kind,
        crate::syntax::TokenKind::Number | crate::syntax::TokenKind::String
    ) || value.is("true")
        || value.is("false")
    {
        return true;
    }
    for declaration in &module.declarations {
        if let Declaration::Variable(local) = declaration {
            if local.name == value.text {
                return fresh_variable(module, local, project, seen, depth + 1);
            }
        }
        if let Declaration::Import(import) = declaration {
            if let Some(binding) = import
                .bindings
                .iter()
                .find(|binding| binding.local == value.text)
            {
                let source = project
                    .resolutions
                    .get(&(module.id.clone(), import.specifier.clone()))
                    .and_then(|id| project.modules.get(id));
                if let Some(source) = source {
                    let local = exported_local(source, &binding.imported);
                    if let Some(variable) =
                        source
                            .declarations
                            .iter()
                            .find_map(|declaration| match declaration {
                                Declaration::Variable(variable) if variable.name == local => {
                                    Some(variable)
                                }
                                _ => None,
                            })
                    {
                        return fresh_variable(source, variable, project, seen, depth + 1);
                    }
                }
            }
        }
    }
    false
}
