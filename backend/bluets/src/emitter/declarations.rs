// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Deterministic declaration emission and retained local export names.

use super::*;

pub(super) fn emit_declaration(
    module: &Module,
    symbols: &[crate::checker::Symbol],
    inferred: Option<&inferred_declarations::Context<'_>>,
    options: &CompilerOptions,
) -> Result<String, Diagnostic> {
    let mut output = String::new();
    let enum_evaluations = crate::enum_eval::evaluate_enums(module);
    let mut enum_position = 0usize;
    let mut private_alias = false;
    let computed_key_dependencies = classes::computed_key_dependencies(module);
    for declaration in &module.declarations {
        let enum_index = enum_position;
        if matches!(declaration, Declaration::Enum(_)) {
            enum_position += 1;
        }
        let comments = output_options::leading_jsdoc(&module.source, declaration.span().start);
        if options.strip_internal
            && comments
                .iter()
                .any(|comment| output_options::is_internal(comment))
        {
            continue;
        }
        let start = output.len();
        if !options.remove_comments {
            for comment in &comments {
                output.push_str(comment);
                output.push('\n');
            }
        }
        let body_start = output.len();
        match declaration {
            Declaration::Import(import) => {
                if let Some(text) = inferred.and_then(|context| context.import(import)) {
                    output.push_str(&text);
                }
            }
            Declaration::TypeAlias(alias)
                if alias.exported
                    || inferred
                        .and_then(|context| context.private_alias(&alias.name))
                        .is_some() =>
            {
                private_alias |= !alias.exported && !explicit_export_name(module, &alias.name);
                output.push_str(if alias.exported {
                    "export type "
                } else {
                    "type "
                });
                output.push_str(&alias.name);
                emit_type_parameters(&mut output, &alias.type_parameters);
                output.push_str(" = ");
                output.push_str(
                    &inferred
                        .and_then(|context| context.private_alias(&alias.name))
                        .map(str::to_string)
                        .unwrap_or_else(|| {
                            declaration_alias_to_ts(&alias.value, &module.source, options)
                        }),
                );
                output.push_str(";\n");
            }
            Declaration::Interface(interface)
                if interface.exported
                    || inferred.is_some_and(|context| context.retained(&interface.name)) =>
            {
                private_alias |=
                    !interface.exported && !explicit_export_name(module, &interface.name);
                output.push_str(if interface.default_export {
                    "export default interface "
                } else if interface.exported {
                    "export interface "
                } else {
                    "interface "
                });
                output.push_str(&interface.name);
                emit_type_parameters(&mut output, &interface.type_parameters);
                if !interface.heritage.is_empty() {
                    output.push_str(" extends ");
                    output.push_str(
                        &interface
                            .heritage
                            .iter()
                            .map(type_to_ts)
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                }
                output.push(' ');
                output.push_str(&callable_objects::render_interface(
                    interface,
                    &module.source,
                    options,
                ));
                output.push('\n');
            }
            Declaration::Variable(variable)
                if !variable.declared
                    && (variable.exported
                        || is_default_export_name(module, &variable.name)
                        || is_value_export_name(module, &variable.name)
                        || computed_key_dependencies.contains(&variable.name)) =>
            {
                private_alias |=
                    !variable.exported && computed_key_dependencies.contains(&variable.name);
                if variable.exported && module.id.ends_with(".js") {
                    output.push_str("export ");
                } else if variable.exported {
                    output.push_str("export declare ");
                } else {
                    output.push_str("declare ");
                }
                output.push_str(variable.kind.as_str());
                output.push(' ');
                output.push_str(&variable.name);
                if variable.annotation.is_none() {
                    if let Some((text, initializer)) =
                        inferred.and_then(|context| context.variable(variable))
                    {
                        output.push_str(if *initializer && !module.id.ends_with(".js") {
                            " = "
                        } else {
                            ": "
                        });
                        output.push_str(text);
                        output.push_str(";\n");
                        continue;
                    }
                }
                let inferred = symbols
                    .iter()
                    .find(|symbol| {
                        symbol.kind == crate::checker::SymbolKind::Variable
                            && symbol.name == variable.name
                            && symbol.span == variable.span
                    })
                    .and_then(|symbol| symbol.value_type.as_ref());
                let value_type = variable.annotation.as_ref().or(inferred);
                if variable.annotation.is_none()
                    && variable.kind == crate::parser::VariableKind::Const
                    && matches!(value_type, Some(Type::Literal(_)))
                {
                    output.push_str(" = ");
                } else {
                    output.push_str(": ");
                }
                output.push_str(
                    &value_type
                        .map(declaration_type_to_ts)
                        .unwrap_or_else(|| "unknown".to_string()),
                );
                output.push_str(";\n");
            }
            Declaration::Function(function)
                if (function.exported
                    || is_default_export_name(module, &function.name)
                    || is_value_export_name(module, &function.name))
                    && (function.overload
                        || !module.declarations.iter().any(|declaration| {
                            matches!(
                                declaration,
                                Declaration::Function(other)
                                    if other.name == function.name && other.overload
                            )
                        })) =>
            {
                if function.default_export {
                    output.push_str("export default function ");
                } else if function.exported && module.id.ends_with(".js") {
                    output.push_str("export function ");
                } else if function.exported {
                    output.push_str("export declare function ");
                } else {
                    output.push_str("declare function ");
                }
                if !function.anonymous {
                    output.push_str(&function.name);
                }
                emit_type_parameters(&mut output, &function.type_parameters);
                output.push('(');
                for (index, parameter) in function.parameters.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    if parameter.rest {
                        output.push_str("...");
                    }
                    output.push_str(&parameter.name);
                    if parameter.optional {
                        output.push('?');
                    }
                    output.push_str(": ");
                    output.push_str(
                        &inferred
                            .and_then(|context| context.parameter_type(parameter.span.start))
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                parameter
                                    .annotation
                                    .as_ref()
                                    .map(type_to_ts)
                                    .unwrap_or_else(|| {
                                        if module.id.ends_with(".js") {
                                            "any".to_string()
                                        } else {
                                            "unknown".to_string()
                                        }
                                    })
                            }),
                    );
                }
                output.push_str("): ");
                output.push_str(
                    &inferred
                        .and_then(|context| context.return_type(function.span.start))
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            function
                                .return_type
                                .as_ref()
                                .map(type_to_ts)
                                .unwrap_or_else(|| "unknown".to_string())
                        }),
                );
                output.push_str(";\n");
            }
            Declaration::Enum(declaration)
                if declaration.exported || is_value_export_name(module, &declaration.name) =>
            {
                let evaluation = enum_evaluations
                    .get(enum_index)
                    .expect("every enum was evaluated");
                output.push_str(&enums::emit_enum_declaration(
                    declaration,
                    evaluation,
                    &module.source,
                    options,
                ));
            }
            Declaration::Namespace(namespace)
                if namespace.exported || is_value_export_name(module, &namespace.name) =>
            {
                let prefix = if namespace.exported {
                    "export declare "
                } else {
                    "declare "
                };
                output.push_str(&namespaces::emit_namespace_declaration(
                    module, namespace, prefix, symbols, inferred, options,
                )?);
            }
            Declaration::Class(class)
                if class.exported
                    || is_default_export_name(module, &class.name)
                    || is_value_export_name(module, &class.name)
                    || computed_key_dependencies.contains(&class.name)
                    || inferred.is_some_and(|context| context.retained(&class.name)) =>
            {
                private_alias |= !class.exported
                    && !explicit_export_name(module, &class.name)
                    && (computed_key_dependencies.contains(&class.name)
                        || inferred.is_some_and(|context| context.retained(&class.name)));
                let prefix = if class.default_export {
                    "export default "
                } else if class.exported {
                    "export declare "
                } else {
                    "declare "
                };
                classes::emit_class_declaration(
                    class,
                    prefix,
                    &mut output,
                    inferred,
                    &module.source,
                    options,
                )?;
            }
            Declaration::UmdExport(export) => {
                output.push_str(&format!("export as namespace {};\n", export.name));
            }
            Declaration::TypeExport(export) => {
                output.push_str("export type ");
                if export.star {
                    output.push('*');
                    if let Some(name) = &export.namespace {
                        output.push_str(" as ");
                        output.push_str(&declaration_name(module, &export.span, name));
                    }
                } else {
                    output.push_str("{ ");
                    emit_export_bindings(module, &export.bindings, false, &mut output);
                    output.push_str(" }");
                }
                if let Some(specifier) = &export.specifier {
                    output.push_str(" from ");
                    if let Some(raw) = export
                        .specifier_span
                        .as_ref()
                        .and_then(|span| module.source.get(span.start..span.end))
                    {
                        output.push_str(raw);
                    } else {
                        output.push_str(&serde_json::to_string(specifier).expect("module string"));
                    }
                }
                emit_import_attributes(module, export.attributes.as_ref(), &mut output);
                output.push_str(";\n");
            }
            Declaration::DefaultExport(export) => {
                output.push_str("export default ");
                output.push_str(&export.name);
                output.push_str(";\n");
            }
            Declaration::ValueExport(export) if export.export_assignment => {
                output.push_str(&format!("export = {};\n", export.bindings[0].local));
            }
            Declaration::ValueExport(export) => {
                if export.star {
                    output.push_str("export *");
                    if let Some(name) = &export.namespace {
                        output.push_str(&format!(" as {name}"));
                    }
                    output.push_str(&format!(" from \"{}\"", export.specifier.as_ref().unwrap()));
                    output.push_str(";\n");
                    continue;
                }
                if export.bindings.is_empty() && export.specifier.is_none() {
                    output.push_str("export {};\n");
                    continue;
                }
                output.push_str("export { ");
                emit_export_bindings(module, &export.bindings, true, &mut output);
                output.push_str(" }");
                if let Some(specifier) = &export.specifier {
                    output.push_str(&format!(" from \"{specifier}\""));
                }
                output.push_str(";\n");
            }
            _ => {}
        }
        if output.len() == body_start {
            output.truncate(start);
        }
    }
    if private_alias
        || (output.is_empty()
            && module.declarations.iter().any(|declaration| {
                matches!(
                    declaration,
                    Declaration::Import(_)
                        | Declaration::TypeExport(_)
                        | Declaration::ValueExport(_)
                        | Declaration::DefaultExport(_)
                )
            }))
    {
        output.push_str("export {};\n");
    }
    Ok(output)
}

fn emit_import_attributes(
    module: &Module,
    attributes: Option<&crate::ImportAttributes>,
    output: &mut String,
) {
    if let Some(attributes) = attributes {
        output.push(' ');
        output.push_str(&module.source[attributes.span.start..attributes.span.end]);
    }
}

fn emit_export_bindings(
    module: &Module,
    bindings: &[crate::parser::ValueExportBinding],
    inline: bool,
    output: &mut String,
) {
    for (index, binding) in bindings.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        if inline && binding.type_only {
            output.push_str("type ");
        }
        output.push_str(&declaration_name(module, &binding.span, &binding.local));
        if binding.local != binding.exported {
            output.push_str(" as ");
            output.push_str(&declaration_name(module, &binding.span, &binding.exported));
        }
    }
}

pub(super) fn declaration_name(module: &Module, span: &SourceSpan, name: &str) -> String {
    crate::lex(&module.id, &module.source[span.start..span.end])
        .ok()
        .and_then(|tokens| {
            tokens
                .into_iter()
                .find(|token| crate::syntax::string_contents(token).as_deref() == Some(name))
        })
        .map_or_else(|| name.to_string(), |token| token.text)
}

pub(super) fn is_default_export_name(module: &Module, name: &str) -> bool {
    module.declarations.iter().any(|declaration| {
        matches!(declaration, Declaration::DefaultExport(export) if export.name == name)
    })
}

fn explicit_export_name(module: &Module, name: &str) -> bool {
    is_default_export_name(module, name) || is_value_export_name(module, name)
        || module.declarations.iter().any(|declaration| {
            matches!(declaration, Declaration::TypeExport(export) if export.specifier.is_none() && export.bindings.iter().any(|binding| binding.local == name))
        })
}

pub(super) fn is_value_export_name(module: &Module, name: &str) -> bool {
    module.declarations.iter().any(|declaration| {
        matches!(declaration, Declaration::ValueExport(export) if export.bindings.iter().any(|binding| binding.local == name))
    })
}

pub(super) fn emit_type_parameters(output: &mut String, parameters: &[TypeParameter]) {
    if !parameters.is_empty() {
        output.push('<');
        for (index, parameter) in parameters.iter().enumerate() {
            if index > 0 {
                output.push_str(", ");
            }
            if parameter.is_const {
                output.push_str("const ");
            }
            output.push_str(parameter.variance.map_or("", |value| value.prefix()));
            output.push_str(&parameter.name);
            if let Some(constraint) = &parameter.constraint {
                output.push_str(" extends ");
                output.push_str(&type_to_ts(constraint));
            }
            if let Some(default) = &parameter.default {
                output.push_str(" = ");
                output.push_str(&type_to_ts(default));
            }
        }
        output.push('>');
    }
}

fn declaration_type_to_ts(value: &Type) -> String {
    match value {
        Type::Record(fields) if !fields.is_empty() => callable_objects::render(fields, &[]),
        _ => type_to_ts(value),
    }
}

fn declaration_alias_to_ts(value: &Type, source: &str, options: &CompilerOptions) -> String {
    callable_objects::render_alias(value, source, options)
        .unwrap_or_else(|| declaration_type_to_ts(value))
}
