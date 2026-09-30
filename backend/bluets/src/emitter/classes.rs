// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class erasure edits and declaration output.
//!
//! Only fully structured classes (constructors, methods and public fields)
//! reach the emitter,
//! so JavaScript output is the original text with type annotations already
//! erased by the parser plus the overload signatures erased here, and
//! declaration output is derived from the same parsed members.

use super::{type_to_ts, Module, TextEdit};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{
    widen_literal_tokens, ClassDeclaration, ClassMemberShell, Declaration, Parameter, Type,
    Visibility,
};

/// Erase every constructor and method overload signature: a declaration with
/// no body has no JavaScript form.
pub(super) fn overload_signature_erasures(module: &Module) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    for declaration in &module.declarations {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        for member in &class.members {
            let signature = member
                .constructor
                .as_ref()
                .is_some_and(|constructor| constructor.body.is_none())
                || member
                    .method
                    .as_ref()
                    .is_some_and(|method| method.body.is_none());
            if signature {
                edits.push(TextEdit {
                    start: member.span.start,
                    end: member.span.end,
                    replacement: String::new(),
                });
            }
        }
    }
    edits
}

/// A literal spelled for a declaration file: TypeScript prints strings with
/// double quotes.
fn declaration_literal(literal: &str, span: &SourceSpan) -> Result<String, Diagnostic> {
    let Some(inner) = literal
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    else {
        return Ok(literal.to_string());
    };
    if inner.contains(['"', '\\']) {
        return Err(Diagnostic::error(
            DiagnosticCode::UnsupportedSyntax,
            span.clone(),
            "declaration output for this string literal field needs an annotation",
        ));
    }
    Ok(format!("\"{inner}\""))
}

/// Whether the class declares any `#name` member.
fn declares_private_name(class: &ClassDeclaration) -> bool {
    class.members.iter().any(|member| {
        member
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with('#'))
    })
}

/// A declaration file writes `public` as nothing.
fn visibility_prefix(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Public => "",
        Visibility::Protected => "protected ",
        Visibility::Private => "private ",
    }
}

/// An optional property's type includes `undefined`, which TypeScript prints
/// for the property and for its constructor parameter.
fn with_undefined_if(value: Type, optional: bool) -> Type {
    if !optional {
        return value;
    }
    match value {
        Type::Union(members) if members.contains(&Type::Undefined) => Type::Union(members),
        Type::Union(mut members) => {
            members.push(Type::Undefined);
            Type::Union(members)
        }
        other => Type::Union(vec![other, Type::Undefined]),
    }
}

/// A constructor's parameter list; an optional parameter property is printed
/// with its `undefined`.
fn constructor_parameters_to_ts(constructor: &crate::parser::ClassConstructor) -> String {
    let mut parameters = constructor.parameters.clone();
    for property in &constructor.parameter_properties {
        let parameter = &mut parameters[property.parameter_index];
        if parameter.optional && parameter.default.is_none() {
            parameter.annotation = parameter
                .annotation
                .take()
                .map(|annotation| with_undefined_if(annotation, true));
        }
    }
    parameters_to_ts(&parameters)
}

fn parameters_to_ts(parameters: &[Parameter]) -> String {
    let rendered = parameters
        .iter()
        .map(|parameter| {
            format!(
                "{}{}{}: {}",
                if parameter.rest { "..." } else { "" },
                parameter.name,
                if parameter.optional || parameter.default.is_some() {
                    "?"
                } else {
                    ""
                },
                // An unannotated parameter with a literal default has the
                // default's widened type, as TypeScript infers it.
                parameter
                    .annotation
                    .clone()
                    .or_else(|| {
                        parameter
                            .default
                            .as_deref()
                            .and_then(|tokens| widen_literal_tokens(tokens, false))
                    })
                    .map(|annotation| type_to_ts(&annotation))
                    .unwrap_or_else(|| "unknown".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("({rendered})")
}

/// The members contributing a declaration line for one constructor or method
/// group: its overload signatures, or its implementation when it has none.
fn visible<'a>(
    class: &'a ClassDeclaration,
    signatures: &[usize],
    implementation: Option<usize>,
) -> Vec<&'a ClassMemberShell> {
    let indices: Vec<usize> = if signatures.is_empty() {
        implementation.into_iter().collect()
    } else {
        signatures.to_vec()
    };
    indices
        .into_iter()
        .map(|index| &class.members[index])
        .collect()
}

pub(super) fn emit_class_declaration(
    class: &ClassDeclaration,
    prefix: &str,
    output: &mut String,
) -> Result<(), Diagnostic> {
    output.push_str(prefix);
    output.push_str("class ");
    output.push_str(&class.name);
    if let Some(base) = &class.extends_name {
        output.push_str(" extends ");
        output.push_str(base);
    }
    output.push_str(" {\n");
    if declares_private_name(class) {
        output.push_str("    #private;\n");
    }
    for field in class.parameter_property_fields() {
        output.push_str("    ");
        output.push_str(visibility_prefix(field.visibility));
        if field.readonly {
            output.push_str("readonly ");
        }
        output.push_str(&field.name);
        if field.optional {
            output.push('?');
        }
        if field.visibility == Visibility::Private {
            output.push_str(";\n");
            continue;
        }
        let Some(value) = field.declared_type() else {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                field.span.clone(),
                "declaration output requires a parameter property type",
            ));
        };
        output.push_str(": ");
        output.push_str(&type_to_ts(&with_undefined_if(value, field.optional)));
        output.push_str(";\n");
    }
    let constructors: Vec<usize> = class
        .members
        .iter()
        .enumerate()
        .filter(|(_, member)| member.constructor.is_some())
        .map(|(index, _)| index)
        .collect();
    let constructor_signatures: Vec<usize> = constructors
        .iter()
        .copied()
        .filter(|&index| {
            class.members[index]
                .constructor
                .as_ref()
                .is_some_and(|constructor| constructor.body.is_none())
        })
        .collect();
    for (index, member) in class.members.iter().enumerate() {
        // A private name is declared once, as `#private;`, not member by member.
        if member
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with('#'))
        {
            continue;
        }
        if member.constructor.is_some() {
            // One block at the first constructor member.
            if constructors.first() != Some(&index) {
                continue;
            }
            let implementation = constructors.iter().copied().find(|&candidate| {
                class.members[candidate]
                    .constructor
                    .as_ref()
                    .is_some_and(|constructor| constructor.body.is_some())
            });
            for shell in visible(class, &constructor_signatures, implementation) {
                let constructor = shell.constructor.as_ref().expect("constructor member");
                output.push_str("    ");
                output.push_str(visibility_prefix(constructor.visibility));
                output.push_str("constructor");
                // TypeScript prints a private constructor without its
                // parameters, and only once.
                if constructor.visibility == Visibility::Private {
                    output.push_str("();\n");
                    break;
                }
                output.push_str(&constructor_parameters_to_ts(constructor));
                output.push_str(";\n");
            }
        } else if let Some(field) = &member.field {
            let Some(value) = field.declared_type() else {
                return Err(Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    field.span.clone(),
                    "declaration output requires a class field type",
                ));
            };
            output.push_str("    ");
            output.push_str(visibility_prefix(field.visibility));
            if field.is_static {
                output.push_str("static ");
            }
            if field.readonly {
                output.push_str("readonly ");
            }
            output.push_str(&field.name);
            if field.optional {
                output.push('?');
            }
            // A private member's type is not part of its declaration.
            if field.visibility == Visibility::Private {
                output.push_str(";\n");
                continue;
            }
            // TypeScript prints a `readonly` field with a literal initializer
            // and no annotation as its literal value, not as a type.
            if let (true, None, Type::Literal(literal)) =
                (field.readonly, &field.annotation, &value)
            {
                output.push_str(" = ");
                output.push_str(&declaration_literal(literal, &field.span)?);
            } else {
                output.push_str(": ");
                output.push_str(&type_to_ts(&value));
            }
            output.push_str(";\n");
        } else if let Some(accessor) = &member.accessor {
            output.push_str("    ");
            output.push_str(visibility_prefix(accessor.visibility));
            if accessor.is_static {
                output.push_str("static ");
            }
            output.push_str(if accessor.getter { "get " } else { "set " });
            output.push_str(&accessor.name);
            if accessor.visibility == Visibility::Private {
                // A private accessor is declared by name only; TypeScript
                // names a setter's parameter `value`.
                output.push_str(if accessor.getter {
                    "();\n"
                } else {
                    "(value);\n"
                });
                continue;
            }
            if accessor.getter {
                let result = accessor.return_type.clone().or_else(|| {
                    class
                        .members
                        .iter()
                        .filter_map(|other| other.accessor.as_ref())
                        .find(|other| {
                            !other.getter
                                && other.name == accessor.name
                                && other.is_static == accessor.is_static
                        })
                        .and_then(|setter| setter.parameters.first())
                        .and_then(|parameter| parameter.annotation.clone())
                });
                let Some(result) = result else {
                    return Err(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        accessor.span.clone(),
                        "declaration output requires an explicit getter return type",
                    ));
                };
                output.push_str("(): ");
                output.push_str(&type_to_ts(&result));
                output.push_str(";\n");
            } else {
                output.push_str(&parameters_to_ts(&accessor.parameters));
                output.push_str(";\n");
            }
        } else if member.method.is_some() {
            let Some(group) = class.method_groups.iter().find(|group| {
                group.signature_member_indices.contains(&index)
                    || group.implementation_member_index == Some(index)
            }) else {
                continue;
            };
            let first = group
                .signature_member_indices
                .iter()
                .copied()
                .chain(group.implementation_member_index)
                .min();
            if first != Some(index) {
                continue;
            }
            for shell in visible(
                class,
                &group.signature_member_indices,
                group.implementation_member_index,
            ) {
                let method = shell.method.as_ref().expect("method member");
                if method.visibility == Visibility::Private {
                    // A private method is declared by name only, once.
                    output.push_str("    private ");
                    if method.is_static {
                        output.push_str("static ");
                    }
                    output.push_str(&method.name);
                    output.push_str(";\n");
                    break;
                }
                let Some(result) = &method.return_type else {
                    return Err(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        SourceSpan::new(&method.span.module, method.span.start, method.span.end),
                        "declaration output requires an explicit class method return type",
                    ));
                };
                output.push_str("    ");
                output.push_str(visibility_prefix(method.visibility));
                if method.is_static {
                    output.push_str("static ");
                }
                output.push_str(&method.name);
                output.push_str(&parameters_to_ts(&method.parameters));
                output.push_str(": ");
                output.push_str(&type_to_ts(result));
                output.push_str(";\n");
            }
        }
    }
    output.push_str("}\n");
    Ok(())
}
