// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Metadata reads only the closed owner-selected graph and retains actual value imports.

use super::*;
use std::collections::BTreeSet;

impl Lowerer<'_, '_> {
    /// TypeScript's `serializeTypeNode` for the types BlueTS resolves.
    pub(super) fn serialize(
        &self,
        class: &ClassDeclaration,
        value: &Type,
        span: &SourceSpan,
    ) -> Result<String, Diagnostic> {
        self.serialize_at(class, value, span, 0)
    }

    fn serialize_at(
        &self,
        class: &ClassDeclaration,
        value: &Type,
        span: &SourceSpan,
        depth: usize,
    ) -> Result<String, Diagnostic> {
        if depth > 8 {
            return Ok("Object".to_string());
        }
        Ok(match value {
            Type::Predicate(predicate) => {
                self.serialize_at(class, &predicate.runtime_type(), span, depth + 1)?
            }
            Type::Number => "Number".to_string(),
            Type::String => "String".to_string(),
            Type::Boolean => "Boolean".to_string(),
            Type::Literal(text) => {
                if text.starts_with(['"', '\'', '`']) {
                    "String".to_string()
                } else if matches!(text.as_str(), "true" | "false") {
                    "Boolean".to_string()
                } else {
                    "Number".to_string()
                }
            }
            Type::Void | Type::Undefined | Type::Null | Type::Never => "void 0".to_string(),
            Type::Any
            | Type::Unknown
            | Type::StrictUnknown
            | Type::BigInt
            | Type::Symbol
            | Type::UniqueSymbol(_)
            | Type::ConstAssertion
            | Type::Readonly(_)
            | Type::IndexedRecord { .. }
            | Type::Record(_)
            | Type::CallableRecord { .. }
            | Type::Intersection(_)
            | Type::Conditional(_)
            | Type::Infer(_)
            | Type::Mapped(_)
            | Type::TemplateLiteral(_)
            | Type::KeyOf(_)
            | Type::IndexedAccess { .. } => "Object".to_string(),
            Type::Array(_) | Type::Tuple(_) => "Array".to_string(),
            Type::Function { .. } | Type::GenericFunction { .. } => "Function".to_string(),
            Type::Union(parts) => {
                // `null` and `undefined` do not count; one remaining kind decides.
                let mut kinds: Vec<String> = Vec::new();
                for part in parts {
                    if matches!(part, Type::Null | Type::Undefined) {
                        continue;
                    }
                    let serialized = self.serialize_at(class, part, span, depth + 1)?;
                    if serialized == "Object" {
                        return Ok("Object".to_string());
                    }
                    if !kinds.contains(&serialized) {
                        kinds.push(serialized);
                    }
                }
                match kinds.as_slice() {
                    [only] => only.clone(),
                    [] => "void 0".to_string(),
                    _ => "Object".to_string(),
                }
            }
            Type::Named { name, .. } => match name.as_str() {
                "object" => "Object".to_string(),
                "symbol" => "Symbol".to_string(),
                "bigint" => "BigInt".to_string(),
                "Array" | "ReadonlyArray" => "Array".to_string(),
                "Promise" => "Promise".to_string(),
                "Function" => "Function".to_string(),
                _ => self.serialize_named(class, name, span, depth)?,
            },
        })
    }

    fn serialize_named(
        &self,
        class: &ClassDeclaration,
        name: &str,
        span: &SourceSpan,
        depth: usize,
    ) -> Result<String, Diagnostic> {
        if class
            .type_parameters
            .iter()
            .any(|parameter| parameter.name == name)
        {
            return Ok("Object".into());
        }
        let found = resolve_local(self.project, self.module, name, depth).ok_or_else(|| {
            unsupported(
                span,
                format!("decorator metadata cannot resolve the authorized type `{name}`"),
            )
        })?;
        let value = match found.declaration {
            Declaration::Class(_) if found.type_only => "Function".into(),
            Declaration::Class(_) => {
                let (head, tail) = name
                    .split_once('.')
                    .map_or((name, String::new()), |(head, tail)| {
                        (head, format!(".{tail}"))
                    });
                self.references.get(head).map_or_else(
                    || name.to_string(),
                    |reference| format!("{}{tail}", reference.text),
                )
            }
            Declaration::Enum(item) => enum_kind(item),
            Declaration::Interface(_) => "Object".into(),
            Declaration::TypeAlias(alias) => {
                alias_kind(self.project, found.owner, &alias.value, depth + 1)
            }
            _ => {
                return Err(unsupported(
                    span,
                    format!("decorator metadata cannot serialize the authorized type `{name}`"),
                ))
            }
        };
        Ok(value)
    }
}

struct Resolved<'a> {
    owner: &'a Module,
    declaration: &'a Declaration,
    type_only: bool,
}

fn local_name(declaration: &Declaration) -> Option<&str> {
    match declaration {
        Declaration::Class(item) => Some(&item.name),
        Declaration::Enum(item) => Some(&item.name),
        Declaration::Interface(item) => Some(&item.name),
        Declaration::TypeAlias(item) => Some(&item.name),
        _ => None,
    }
}

fn resolve_local<'a>(
    project: &'a crate::Project,
    module: &'a Module,
    name: &str,
    depth: usize,
) -> Option<Resolved<'a>> {
    if depth > 8 {
        return None;
    }
    if let Some(declaration) = module
        .declarations
        .iter()
        .find(|declaration| local_name(declaration) == Some(name))
    {
        return Some(Resolved {
            owner: module,
            declaration,
            type_only: false,
        });
    }
    let (head, tail) = name
        .split_once('.')
        .map_or((name, None), |(head, tail)| (head, Some(tail)));
    for declaration in &module.declarations {
        let Declaration::Import(import) = declaration else {
            continue;
        };
        let Some(binding) = import.bindings.iter().find(|binding| binding.local == head) else {
            continue;
        };
        let provider = project
            .modules
            .get(project.resolved_import(&module.id, import)?)?;
        let exported = if binding.imported == "*" {
            tail?
        } else {
            if tail.is_some() {
                return None;
            }
            &binding.imported
        };
        let mut resolved = resolve_export(project, provider, exported, depth + 1)?;
        resolved.type_only |= import.type_only || binding.type_only;
        return Some(resolved);
    }
    None
}

fn resolve_export<'a>(
    project: &'a crate::Project,
    module: &'a Module,
    name: &str,
    depth: usize,
) -> Option<Resolved<'a>> {
    if depth > 8 {
        return None;
    }
    for declaration in &module.declarations {
        let direct = match declaration {
            Declaration::Class(item) => item.exported && item.export_name() == name,
            Declaration::Enum(item) => item.exported && item.name == name,
            Declaration::Interface(item) => item.exported && item.name == name,
            Declaration::TypeAlias(item) => item.exported && item.name == name,
            _ => false,
        };
        if direct {
            return Some(Resolved {
                owner: module,
                declaration,
                type_only: false,
            });
        }
        if let Declaration::ValueExport(export) = declaration {
            if let Some(binding) = export
                .bindings
                .iter()
                .find(|binding| binding.exported == name)
            {
                let mut found = if let Some(specifier) = &export.specifier {
                    let provider = project
                        .modules
                        .get(project.resolved_module(&module.id, specifier)?)?;
                    resolve_export(project, provider, &binding.local, depth + 1)
                } else {
                    resolve_local(project, module, &binding.local, depth + 1)
                }?;
                found.type_only |= binding.type_only;
                return Some(found);
            }
            if export.star && export.namespace.is_none() {
                if let Some(provider) = export
                    .specifier
                    .as_ref()
                    .and_then(|specifier| project.resolved_module(&module.id, specifier))
                    .and_then(|id| project.modules.get(id))
                {
                    if let Some(found) = resolve_export(project, provider, name, depth + 1) {
                        return Some(found);
                    }
                }
            }
        }
        if let Declaration::TypeExport(export) = declaration {
            if let Some(binding) = export
                .bindings
                .iter()
                .find(|binding| binding.exported == name)
            {
                let mut found = if let Some(provider) = project
                    .resolved_type_export(&module.id, export)
                    .and_then(|id| project.modules.get(id))
                {
                    resolve_export(project, provider, &binding.local, depth + 1)
                } else {
                    resolve_local(project, module, &binding.local, depth + 1)
                }?;
                found.type_only = true;
                return Some(found);
            }
        }
    }
    None
}

fn enum_kind(item: &crate::parser::EnumDeclaration) -> String {
    let numeric = item.members.iter().all(|member| {
        member.initializer.as_ref().is_none_or(|tokens| {
            tokens
                .iter()
                .all(|token| token.kind != crate::TokenKind::String)
        })
    });
    if numeric { "Number" } else { "String" }.into()
}

fn alias_kind(project: &crate::Project, owner: &Module, value: &Type, depth: usize) -> String {
    if depth > 8 {
        return "Object".into();
    }
    match value {
        Type::Number => "Number".into(),
        Type::String => "String".into(),
        Type::Boolean => "Boolean".into(),
        Type::Void | Type::Undefined | Type::Null | Type::Never => "void 0".into(),
        Type::Array(_) | Type::Tuple(_) => "Array".into(),
        Type::Function { .. } | Type::GenericFunction { .. } => "Function".into(),
        Type::Literal(text)
            if text.starts_with(['\'', '"']) || text.starts_with(char::from(96)) =>
        {
            "String".into()
        }
        Type::Literal(text) if matches!(text.as_str(), "true" | "false") => "Boolean".into(),
        Type::Literal(_) => "Number".into(),
        Type::Named { name, .. } => match name.as_str() {
            "Array" | "ReadonlyArray" => "Array".into(),
            "Function" => "Function".into(),
            "Promise" => "Promise".into(),
            _ => match resolve_local(project, owner, name, depth + 1) {
                Some(Resolved {
                    declaration: Declaration::TypeAlias(alias),
                    owner,
                    ..
                }) => alias_kind(project, owner, &alias.value, depth + 1),
                Some(Resolved {
                    declaration: Declaration::Enum(item),
                    ..
                }) => enum_kind(item),
                _ => "Object".into(),
            },
        },
        Type::Union(parts) => {
            let mut kinds = parts
                .iter()
                .filter(|part| !matches!(part, Type::Null | Type::Undefined))
                .map(|part| alias_kind(project, owner, part, depth + 1))
                .collect::<BTreeSet<_>>();
            if kinds.len() == 1 {
                kinds.pop_first().unwrap()
            } else {
                "Object".into()
            }
        }
        _ => "Object".into(),
    }
}

fn retain_type(
    project: &crate::Project,
    module: &Module,
    annotation: &Type,
    names: &mut BTreeSet<String>,
) {
    match annotation {
        Type::Named { name, .. } => {
            if let Some(found) = resolve_local(project, module, name, 0) {
                if !found.type_only
                    && matches!(
                        found.declaration,
                        Declaration::Class(_) | Declaration::Enum(_)
                    )
                {
                    let root = name.split('.').next().unwrap();
                    if module.declarations.iter().any(|declaration| matches!(declaration, Declaration::Import(import) if import.bindings.iter().any(|binding| binding.local == root))) {
                        names.insert(root.to_string());
                    }
                }
            }
        }
        Type::Union(parts) | Type::Intersection(parts) => {
            for part in parts {
                retain_type(project, module, part, names);
            }
        }
        _ => {}
    }
}

pub(super) fn runtime_imports(module: &Module, project: &crate::Project) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for (declaration, _) in super::super::runtime_declarations(&module.declarations) {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        for shell in &class.members {
            if let Some(constructor) = &shell.constructor {
                if lowers_class(class) {
                    for parameter in &constructor.parameters {
                        if let Some(value) = &parameter.annotation {
                            retain_type(project, module, value, &mut names);
                        }
                    }
                }
            }
            if !member_is_decorated(shell) {
                continue;
            }
            if let Some(field) = &shell.field {
                if let Some(value) = field.declared_type() {
                    retain_type(project, module, &value, &mut names);
                }
            }
            if let Some(method) = &shell.method {
                for parameter in &method.parameters {
                    if let Some(value) = &parameter.annotation {
                        retain_type(project, module, value, &mut names);
                    }
                }
                if let Some(value) = &method.return_type {
                    retain_type(project, module, value, &mut names);
                }
            }
            if let Some(accessor) = &shell.accessor {
                for parameter in &accessor.parameters {
                    if let Some(value) = &parameter.annotation {
                        retain_type(project, module, value, &mut names);
                    }
                }
                if let Some(value) = &accessor.return_type {
                    retain_type(project, module, value, &mut names);
                }
            }
        }
    }
    names
}
