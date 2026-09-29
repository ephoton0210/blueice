// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Class erasure edits and declaration output.
//!
//! Only fully structured classes (constructors and methods) reach the emitter,
//! so JavaScript output is the original text with type annotations already
//! erased by the parser plus the overload signatures erased here, and
//! declaration output is derived from the same parsed members.

use super::{type_to_ts, Module, TextEdit};
use crate::diagnostic::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::parser::{ClassDeclaration, ClassMemberShell, Declaration, Parameter};

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
                parameter
                    .annotation
                    .as_ref()
                    .map(type_to_ts)
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
                output.push_str("    constructor");
                output.push_str(&parameters_to_ts(&constructor.parameters));
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
                let Some(result) = &method.return_type else {
                    return Err(Diagnostic::error(
                        DiagnosticCode::UnsupportedSyntax,
                        SourceSpan::new(&method.span.module, method.span.start, method.span.end),
                        "declaration output requires an explicit class method return type",
                    ));
                };
                output.push_str("    ");
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
