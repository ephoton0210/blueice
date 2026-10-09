// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Deterministic declaration emission and retained local export names.

use super::*;

pub(super) fn emit_declaration(
    module: &Module,
    symbols: &[crate::checker::Symbol],
    inferred: Option<&inferred_declarations::Context<'_>>,
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
        match declaration {
            Declaration::Import(import) if import.type_only => {
                output.push_str(&module.source[import.span.start..import.span.end]);
                if !output.ends_with('\n') {
                    output.push('\n');
                }
            }
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
                private_alias |= !alias.exported;
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
                        .unwrap_or_else(|| declaration_type_to_ts(&alias.value)),
                );
                output.push_str(";\n");
            }
            Declaration::Interface(interface)
                if interface.exported
                    || inferred.is_some_and(|context| context.retained(&interface.name)) =>
            {
                private_alias |= !interface.exported;
                output.push_str(if interface.exported {
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
                output.push_str(&callable_objects::render_with_indices(
                    &interface.fields,
                    &interface.signatures,
                    &interface.indices,
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
                if variable.exported {
                    output.push_str("export declare ");
                } else {
                    output.push_str("declare ");
                }
                output.push_str(variable.kind.as_str());
                output.push(' ');
                output.push_str(&variable.name);
                if variable.annotation.is_none() {
                    if let Some((text, initializer)) =
                        inferred.and_then(|context| context.variable(&variable.name))
                    {
                        output.push_str(if *initializer { " = " } else { ": " });
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
                                    .unwrap_or_else(|| "unknown".to_string())
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
                output.push_str(&enums::emit_enum_declaration(declaration, evaluation));
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
                    module, namespace, prefix,
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
                    && (computed_key_dependencies.contains(&class.name)
                        || inferred.is_some_and(|context| context.retained(&class.name)));
                let prefix = if class.default_export {
                    "export default "
                } else if class.exported {
                    "export declare "
                } else {
                    "declare "
                };
                classes::emit_class_declaration(class, prefix, &mut output, inferred)?;
            }
            Declaration::TypeExport(export) => {
                output.push_str("export type ");
                if export.bindings.len() == 1 && export.bindings[0] == "*" {
                    output.push('*');
                } else {
                    output.push_str("{ ");
                    output.push_str(&export.bindings.join(", "));
                    output.push_str(" }");
                }
                if let Some(specifier) = &export.specifier {
                    output.push_str(" from ");
                    output.push_str(&format!("\"{specifier}\""));
                }
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
                    output.push_str(&format!(
                        " from \"{}\";\n",
                        export.specifier.as_ref().unwrap()
                    ));
                    continue;
                }
                if export.bindings.is_empty() && export.specifier.is_none() {
                    output.push_str("export {};\n");
                    continue;
                }
                output.push_str("export { ");
                for (index, binding) in export.bindings.iter().enumerate() {
                    if index > 0 {
                        output.push_str(", ");
                    }
                    output.push_str(&binding.local);
                    if binding.local != binding.exported {
                        output.push_str(" as ");
                        output.push_str(&binding.exported);
                    }
                }
                output.push_str(" }");
                if let Some(specifier) = &export.specifier {
                    output.push_str(&format!(" from \"{specifier}\""));
                }
                output.push_str(";\n");
            }
            _ => {}
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

pub(super) fn is_default_export_name(module: &Module, name: &str) -> bool {
    module.declarations.iter().any(|declaration| {
        matches!(declaration, Declaration::DefaultExport(export) if export.name == name)
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
