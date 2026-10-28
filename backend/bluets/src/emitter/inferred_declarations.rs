// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Names and import retention for declarations inferred from checked values.

use super::*;
use crate::checker::CheckedModule;
use crate::parser::{ImportDeclaration, VariableDeclaration, VariableKind};
use std::collections::BTreeSet;

mod diagnostics;
mod origins;
mod render;
mod unions;
use origins::{
    declared_function, declares_symbol_value, exported_local, fresh_variable, public_type_name,
    record_annotation,
};

pub(super) struct Context<'a> {
    module: &'a Module,
    project: &'a Project,
    used_imports: BTreeSet<String>,
    variables: BTreeMap<usize, (String, bool)>,
    annotations: BTreeMap<usize, String>,
    value_types: BTreeMap<String, Type>,
    return_types: BTreeMap<usize, String>,
    parameter_types: BTreeMap<usize, String>,
    private_aliases: BTreeMap<String, String>,
    interface_fields: BTreeMap<usize, Vec<crate::parser::TypeField>>,
    checking: crate::CheckingOptions,
    max_expansions: usize,
}

impl<'a> Context<'a> {
    pub(super) fn new(
        checked: &'a CheckedModule,
        project: &'a Project,
        order: &literal_order::Order,
        options: &CompilerOptions,
    ) -> Result<Self, Diagnostic> {
        let module = &checked.module;
        let mut context = Self {
            module,
            project,
            used_imports: BTreeSet::new(),
            variables: BTreeMap::new(),
            annotations: BTreeMap::new(),
            value_types: checked
                .symbols
                .iter()
                .filter_map(|symbol| {
                    (symbol.kind == crate::checker::SymbolKind::Variable)
                        .then(|| {
                            symbol
                                .value_type
                                .clone()
                                .map(|value| (symbol.name.clone(), value))
                        })
                        .flatten()
                })
                .collect(),
            return_types: BTreeMap::new(),
            parameter_types: BTreeMap::new(),
            private_aliases: BTreeMap::new(),
            interface_fields: BTreeMap::new(),
            checking: checked.checking,
            max_expansions: options.limits.max_type_expansions,
        };
        for (declaration, _) in super::runtime_declarations(&module.declarations) {
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
                let text = context.render(annotation, 0, &variable.span)?;
                context.annotations.insert(variable.span.start, text);
                continue;
            }
            let value = checked
                .symbols
                .iter()
                .find(|symbol| {
                    symbol.kind == crate::checker::SymbolKind::Variable
                        && symbol.span == variable.span
                })
                .and_then(|symbol| symbol.value_type.as_ref())
                .unwrap_or(&Type::Unknown);
            let expanded = super::class_expressions::declaration_type(
                value,
                &checked.class_expression_surfaces,
                &variable.span,
            )?;
            let expanded =
                crate::checker::tuple_compositions::display_type(&expanded, checked.checking);
            let expanded = order.variable(module, variable, &expanded);
            let expanded = super::inference_literals::declaration_union(
                expanded,
                checked.fresh_variables.contains(&variable.span.start),
            );
            let value = &expanded;
            let value = if variable.kind == VariableKind::Const
                && matches!(value, Type::Named { name, arguments } if name == "symbol" && arguments.is_empty())
                && context.fresh_symbol_initializer(variable)
            {
                "unique symbol".to_string()
            } else if let Some(name) = context.function_initializer(variable) {
                context.retain(&name);
                format!("typeof {name}")
            } else {
                context.render(value, 0, &variable.span)?
            };
            let literal = matches!(
                checked
                    .symbols
                    .iter()
                    .find(|symbol| symbol.span == variable.span)
                    .and_then(|symbol| symbol.value_type.as_ref()),
                Some(Type::Literal(_))
            );
            let primitive = matches!(checked.symbols.iter().find(|symbol| symbol.span == variable.span)
                .and_then(|symbol| symbol.value_type.as_ref()), Some(Type::Literal(text))
                    if text.starts_with(['\'', '"', '`']) || text.parse::<f64>().is_ok() || matches!(text.as_str(), "true" | "false"));
            let initializer = variable.kind == VariableKind::Const
                && literal
                && !value.starts_with("import(")
                && !module.declarations.iter().any(|declaration| matches!(declaration,
                    Declaration::DefaultExport(export) if export.expression && export.name == variable.name))
                && (!primitive
                    || checked.fresh_variables.contains(&variable.span.start)
                    || fresh_variable(module, variable, project, &mut BTreeSet::new(), 0));
            context
                .variables
                .insert(variable.span.start, (value, initializer));
        }
        // Value imports also appear in annotations, class heritage and signatures.
        for (declaration, _) in super::runtime_declarations(&module.declarations) {
            match declaration {
                Declaration::TypeAlias(alias) if alias.exported => {
                    context.references(&alias.value);
                }
                Declaration::TypeExport(export) if export.specifier.is_none() => {
                    for binding in &export.bindings {
                        context.retain(&binding.local);
                    }
                }
                Declaration::ValueExport(export) if export.specifier.is_none() => {
                    for binding in &export.bindings {
                        context.retain(&binding.local);
                    }
                }
                Declaration::Function(function)
                    if function.exported
                        || context.used_imports.contains(&function.name)
                        || is_default_export_name(module, &function.name)
                        || is_value_export_name(module, &function.name) =>
                {
                    for parameter in &function.parameters {
                        if let Some(value) = &parameter.annotation {
                            context.references(value);
                            if record_annotation(value) {
                                let rendered = context.render(value, 0, &parameter.span)?;
                                context
                                    .parameter_types
                                    .insert(parameter.span.start, rendered);
                            }
                        }
                    }
                    if let Some(value) = &function.return_type {
                        context.references(value);
                        if record_annotation(value) {
                            let rendered = context.render(value, 0, &function.span)?;
                            context.return_types.insert(function.span.start, rendered);
                        }
                    } else if let Some(value) = checked.inferred_returns.get(&function.span.start) {
                        let value = super::class_expressions::declaration_type(
                            &order.function(function, value),
                            &checked.class_expression_surfaces,
                            &function.span,
                        )?;
                        let value = super::class_expressions::rename_shadowed_binders(
                            &value,
                            &function.type_parameters,
                        );
                        let rendered = context.render(&value, 0, &function.span)?;
                        context.return_types.insert(function.span.start, rendered);
                    }
                }
                Declaration::Interface(interface)
                    if interface.exported || context.used_imports.contains(&interface.name) =>
                {
                    for parent in &interface.heritage {
                        context.references(parent);
                    }
                    for field in &interface.fields {
                        context.references(&field.value);
                    }
                    let fields = context.render_fields(&interface.fields, 0, &interface.span)?;
                    context
                        .interface_fields
                        .insert(interface.span.start, fields);
                }
                Declaration::Class(class)
                    if class.exported
                        || is_default_export_name(module, &class.name)
                        || is_value_export_name(module, &class.name) =>
                {
                    if let Some(name) = &class.extends_name {
                        context.retain(name);
                    }
                    for (value, _) in &class.implements {
                        context.references(value);
                    }
                    for member in &class.members {
                        if let Some(field) = &member.field {
                            if let Some(value) = &field.annotation {
                                context.references(value);
                            }
                        }
                        if let Some(accessor) = &member.accessor {
                            for parameter in &accessor.parameters {
                                if let Some(value) = &parameter.annotation {
                                    context.references(value);
                                }
                            }
                            let value = accessor.return_type.clone().or_else(|| {
                                checked
                                    .inferred_returns
                                    .get(&accessor.span.start)
                                    .map(super::inferred_returns::getter)
                            });
                            if let Some(value) = value {
                                let rendered = context.render(&value, 4, &accessor.span)?;
                                context.return_types.insert(accessor.span.start, rendered);
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
                                if record_annotation(value) {
                                    let rendered = context.render(value, 4, &method.span)?;
                                    context.return_types.insert(method.span.start, rendered);
                                }
                            } else if let Some(value) =
                                checked.inferred_returns.get(&method.span.start)
                            {
                                context.references(value);
                                if record_annotation(value) {
                                    let rendered = context.render(value, 4, &method.span)?;
                                    context.return_types.insert(method.span.start, rendered);
                                }
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
        for _ in 0..module.declarations.len() {
            let before = (context.private_aliases.len(), context.used_imports.len());
            for declaration in &module.declarations {
                if let Declaration::Interface(interface) = declaration {
                    if interface.exported || context.used_imports.contains(&interface.name) {
                        for parent in &interface.heritage {
                            context.references(parent);
                        }
                        for field in &interface.fields {
                            context.references(&field.value);
                        }
                    }
                }
                if let Declaration::Variable(variable) = declaration {
                    if context.used_imports.contains(&variable.name)
                        && !context.variables.contains_key(&variable.span.start)
                    {
                        if let Some(value) = context.value_types.get(&variable.name) {
                            if let Type::CallableRecord { signatures, .. } = value.object_type() {
                                if let Some(name) = signatures.iter().find_map(|signature| {
                                    match &signature.result {
                                        Type::Named { name, .. } if signature.construct => {
                                            Some(name.clone())
                                        }
                                        _ => None,
                                    }
                                }) {
                                    let value = Type::Named {
                                        name,
                                        arguments: Vec::new(),
                                    };
                                    context.references(&value);
                                    let rendered = context.render(&value, 0, &variable.span)?;
                                    context.variables.insert(
                                        variable.span.start,
                                        (format!("typeof {rendered}"), false),
                                    );
                                }
                            }
                        }
                    }
                }
                if let Declaration::Class(class) = declaration {
                    if context.used_imports.contains(&class.name) {
                        if let Some(parent) = &class.extends_name {
                            context.retain(parent);
                        }
                    }
                }
                if let Declaration::TypeAlias(alias) = declaration {
                    if !alias.exported
                        && context.used_imports.contains(&alias.name)
                        && !context.private_aliases.contains_key(&alias.name)
                    {
                        context.references(&alias.value);
                        let rendered = context.render(&alias.value, 0, &alias.span)?;
                        context.private_aliases.insert(alias.name.clone(), rendered);
                    }
                }
            }
            if (context.private_aliases.len(), context.used_imports.len()) == before {
                break;
            }
        }
        Ok(context)
    }

    pub(super) fn retained(&self, name: &str) -> bool {
        self.used_imports.contains(name)
    }

    pub(super) fn interface_fields(&self, start: usize) -> Option<&[crate::parser::TypeField]> {
        self.interface_fields.get(&start).map(Vec::as_slice)
    }

    pub(super) fn private_alias(&self, name: &str) -> Option<&str> {
        self.private_aliases.get(name).map(String::as_str)
    }

    pub(super) fn variable(&self, variable: &VariableDeclaration) -> Option<&(String, bool)> {
        self.variables.get(&variable.span.start)
    }

    pub(super) fn annotation(&self, start: usize) -> Option<&str> {
        self.annotations.get(&start).map(String::as_str)
    }

    /// A broad computed method name is represented as a function-valued property.
    pub(super) fn computed_method_property(
        &self,
        member: &crate::parser::ClassMemberShell,
    ) -> bool {
        let [open, name, close] = member.key.as_slice() else {
            return false;
        };
        open.is("[")
            && close.is("]")
            && name.kind == crate::TokenKind::Identifier
            && self
                .value_types
                .get(&name.text)
                .is_some_and(|value| !matches!(value, Type::Literal(_) | Type::UniqueSymbol(_)))
    }

    pub(super) fn return_type(&self, start: usize) -> Option<&str> {
        self.return_types.get(&start).map(String::as_str)
    }

    pub(super) fn parameter_type(&self, start: usize) -> Option<&str> {
        self.parameter_types.get(&start).map(String::as_str)
    }

    fn fresh_symbol_initializer(&self, variable: &VariableDeclaration) -> bool {
        let tokens = &variable.initializer;
        let direct = tokens.first().is_some_and(|token| token.is("Symbol"))
            && (tokens.get(1).is_some_and(|token| token.is("("))
                || tokens.get(1).is_some_and(|token| token.is("."))
                    && tokens.get(2).is_some_and(|token| token.is("for"))
                    && tokens.get(3).is_some_and(|token| token.is("(")));
        direct
            && !declares_symbol_value(self.module)
            && !self.project.ambient_declaration_modules.iter().any(|id| {
                self.project
                    .modules
                    .get(id)
                    .is_some_and(declares_symbol_value)
            })
    }

    fn retain(&mut self, name: &str) {
        let bare = name
            .strip_prefix("typeof ")
            .map(|query| query.split('@').next().unwrap_or(query))
            .unwrap_or(name);
        if !bare.contains('@') {
            self.used_imports
                .insert(bare.split('.').next().unwrap_or(bare).to_string());
        }
    }

    fn references(&mut self, value: &Type) {
        if let Some(children) = value.operator_children() {
            for child in children {
                self.references(child);
            }
            return;
        }
        match value {
            Type::CallableRecord { fields, signatures } => {
                self.references(&Type::Record(fields.clone()));
                for signature in signatures {
                    self.references(&signature.function_type());
                }
            }
            Type::KeyOf(value) => self.references(value),
            Type::IndexedAccess { object, index, .. } => {
                self.references(object);
                self.references(index);
            }
            Type::GenericFunction {
                type_parameters,
                parameters,
                result,
                ..
            } => {
                let previous = self.used_imports.clone();
                self.references(&Type::Function {
                    parameters: parameters.clone(),
                    result: result.clone(),
                });
                for parameter in type_parameters {
                    if let Some(constraint) = &parameter.constraint {
                        self.references(constraint);
                    }
                    if let Some(default) = &parameter.default {
                        self.references(default);
                    }
                }
                for parameter in type_parameters {
                    if !previous.contains(&parameter.name) {
                        self.used_imports.remove(&parameter.name);
                    }
                }
            }
            Type::Predicate(predicate) => {
                if let Some(target) = &predicate.target {
                    self.references(target);
                }
            }
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
                    if let Some(entity) = crate::symbol_keys::entity(&field.name) {
                        self.retain(entity);
                    }
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
        if declared_function(self.module, &name, self.project, &mut BTreeSet::new(), 0) {
            return Some(name);
        }
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
        let resolved = self.project.resolved_import(&self.module.id, import.0)?;
        let source = self.project.modules.get(resolved)?;
        let exported = if import.1.imported == "*" {
            name.split_once('.')?.1
        } else {
            &import.1.imported
        };
        let local = exported_local(source, exported);
        declared_function(source, &local, self.project, &mut BTreeSet::new(), 0).then_some(name)
    }

    pub(super) fn import(&self, import: &ImportDeclaration) -> Option<String> {
        let specifier = &self.module.source[import.specifier_span.start..import.specifier_span.end];
        if !import.type_only
            && import.bindings.is_empty()
            && crate::lex(
                &self.module.id,
                &self.module.source[import.span.start..import.specifier_span.start],
            )
            .is_ok_and(|tokens| {
                tokens.len() == 2
                    && tokens[0].is("import")
                    && tokens[1].kind == crate::TokenKind::Eof
            })
        {
            return Some(format!("import {specifier};\n"));
        }
        let bindings: Vec<_> = import
            .bindings
            .iter()
            .filter(|binding| self.used_imports.contains(&binding.local))
            .collect();
        if bindings.is_empty() {
            return None;
        }
        if import.equals_require {
            return Some(format!(
                "import {}{} = require({specifier});\n",
                if import.type_only { "type " } else { "" },
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
                let name =
                    declarations::declaration_name(self.module, &binding.span, &binding.imported);
                let text = if binding.local == binding.imported {
                    name
                } else {
                    format!("{name} as {}", binding.local)
                };
                if binding.type_only && !import.type_only {
                    format!("type {text}")
                } else {
                    text
                }
            })
            .collect();
        if !named.is_empty() {
            clauses.push(format!("{{ {} }}", named.join(", ")));
        }
        let attributes = import
            .attributes
            .as_ref()
            .filter(|_| import.type_only)
            .map_or(String::new(), |attributes| {
                format!(
                    " {}",
                    &self.module.source[attributes.span.start..attributes.span.end]
                )
            });
        Some(format!(
            "import {}{} from {specifier}{attributes};\n",
            if import.type_only { "type " } else { "" },
            clauses.join(", ")
        ))
    }
}
