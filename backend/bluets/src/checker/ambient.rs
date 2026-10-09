// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static global augmentation is collected without executable bindings.

use super::*;

mod contexts;
pub(super) use contexts::collect_contexts;
mod modules;
pub(super) use modules::{seed_module_types, seed_module_values};
mod augmentations;
pub(super) use augmentations::merge_module_types;

pub(super) fn insert_body(
    project: &Project,
    declarations: &[Declaration],
    ambient: &mut AmbientDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let shadowed = declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Interface(item) => Some(item.name.clone()),
            Declaration::TypeAlias(item) => Some(item.name.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let contextual = |value: &Type, span: &SourceSpan, parameters: &[TypeParameter]| {
        let substitutions = ambient
            .contexts
            .get(&span.module)
            .into_iter()
            .flat_map(|context| context.iter())
            .filter(|(name, _)| {
                !shadowed.contains(*name)
                    && !parameters.iter().any(|parameter| &parameter.name == *name)
            })
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        substitute_type(value, &substitutions)
    };
    let definitions = declarations
        .iter()
        .map(|declaration| match declaration {
            Declaration::TypeAlias(alias) => Some(contextual(
                &alias.value,
                &alias.span,
                &alias.type_parameters,
            )),
            Declaration::Interface(interface) => Some(contextual(
                &interface_value(interface),
                &interface.span,
                &interface.type_parameters,
            )),
            Declaration::Variable(variable) => Some(contextual(
                &variable.annotation.clone().unwrap_or(Type::Unknown),
                &variable.span,
                &[],
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    for (declaration, definition) in declarations.iter().zip(definitions) {
        match declaration {
            Declaration::TypeAlias(alias) => insert_ambient_type(
                project,
                ambient,
                diagnostics,
                &alias.name,
                TypeDefinition {
                    kind: TypeDefinitionKind::Alias,
                    parameters: alias.type_parameters.clone(),
                    value: definition.expect("type aliases have a contextual definition"),
                },
                &alias.span,
            ),
            Declaration::Interface(interface) => insert_ambient_type(
                project,
                ambient,
                diagnostics,
                &interface.name,
                TypeDefinition {
                    kind: TypeDefinitionKind::Interface,
                    parameters: interface.type_parameters.clone(),
                    value: definition.expect("interfaces have a contextual definition"),
                },
                &interface.span,
            ),
            Declaration::Variable(variable) if variable.declared => {
                insert_ambient_value(
                    ambient,
                    diagnostics,
                    &variable.name,
                    definition.expect("variables have a contextual annotation"),
                    &variable.span,
                );
                if variable.kind == crate::parser::VariableKind::Const {
                    ambient
                        .binding_kinds
                        .insert(variable.name.clone(), scopes::BindingKind::Const);
                }
            }
            Declaration::Function(function) if function.declared || function.overload => {
                insert_ambient_function(ambient, diagnostics, function);
                ambient
                    .binding_kinds
                    .insert(function.name.clone(), scopes::BindingKind::Function);
            }
            _ => {}
        }
    }
}

pub(super) fn has_module_syntax(declarations: &[Declaration]) -> bool {
    crate::parser::has_module_syntax(declarations)
}

pub(super) fn collect_augmentations(
    project: &Project,
    declarations: &[Declaration],
    allow_global: bool,
    ambient: &mut AmbientDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for declaration in declarations {
        match declaration {
            Declaration::Ambient(item) => {
                if let Some(specifier) = &item.specifier {
                    if !allow_global
                        && (specifier.starts_with("./") || specifier.starts_with("../"))
                    {
                        diagnostics.push(
                            Diagnostic::error(
                                DiagnosticCode::InvalidDeclarationFile,
                                item.specifier_span.clone(),
                                "ambient modules in a script cannot use a relative name",
                            )
                            .with_typescript(2436, Vec::new()),
                        );
                    }
                    collect_augmentations(project, &item.body, true, ambient, diagnostics);
                } else if allow_global {
                    insert_body(project, &item.body, ambient, diagnostics);
                    collect_augmentations(project, &item.body, false, ambient, diagnostics);
                } else {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::InvalidDeclarationFile,
                            item.specifier_span.clone(),
                            "global augmentation requires an external module or ambient module body",
                        ).with_typescript(2669, Vec::new()),
                    );
                }
            }
            Declaration::Namespace(item) => {
                collect_augmentations(project, &item.body, false, ambient, diagnostics);
            }
            _ => {}
        }
    }
}

pub(super) fn check_merged_members(
    project: &Project,
    diagnostics: &mut Vec<Diagnostic>,
    earlier: &Type,
    later: &Type,
) {
    let fields = |value: &Type| match value {
        Type::Record(fields) | Type::CallableRecord { fields, .. } => fields.clone(),
        _ => Vec::new(),
    };
    let earlier = fields(earlier);
    for field in fields(later) {
        let Some(old) = earlier.iter().find(|old| old.name == field.name) else {
            continue;
        };
        if !matches!(
            field.value,
            Type::Function { .. } | Type::GenericFunction { .. }
        ) && old.value != field.value
        {
            let span = field_name_span(project, &field);
            let mut diagnostic = Diagnostic::error(
                DiagnosticCode::TypeMismatch,
                span.clone(),
                format!(
                    "subsequent interface property `{}` has a different type",
                    field.name
                ),
            )
            .with_typescript(
                2717,
                vec![
                    field.name.clone(),
                    crate::diagnostic::type_text::render_in(&old.value, project),
                    crate::diagnostic::type_text::render_in(&field.value, project),
                ],
            );
            if let Some(source) = project.source(&span.module) {
                diagnostic = diagnostic.with_source_position(source);
            }
            diagnostics.push(diagnostic);
        }
    }
}

fn field_name_span(project: &Project, field: &TypeField) -> SourceSpan {
    project
        .source(&field.span.module)
        .and_then(|source| source.get(field.span.start..field.span.end))
        .and_then(|source| crate::syntax::lex(&field.span.module, source).ok())
        .and_then(|tokens| {
            tokens.into_iter().find(|token| {
                token.is(&field.name)
                    || crate::syntax::string_contents(token).as_ref() == Some(&field.name)
            })
        })
        .map(|token| {
            SourceSpan::new(
                &field.span.module,
                field.span.start + token.start,
                field.span.start + token.end,
            )
        })
        .unwrap_or_else(|| field.span.clone())
}
