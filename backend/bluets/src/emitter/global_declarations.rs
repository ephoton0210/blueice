// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static global namespaces retain their ambient declaration output.
use super::*;
use crate::parser::{AmbientDeclaration, NamespaceDeclaration};

pub(super) fn emit(
    module: &Module,
    global: &AmbientDeclaration,
    symbols: &[crate::checker::Symbol],
    inferred: Option<&inferred_declarations::Context<'_>>,
    options: &CompilerOptions,
) -> Result<String, Diagnostic> {
    let namespace = NamespaceDeclaration {
        name: "global".into(),
        name_span: global.specifier_span.clone(),
        exported: false,
        declared: true,
        explicit_exports: false,
        implicit: false,
        body: ambient_body(&global.body),
        header_span: global.span.clone(),
        closing_span: global.span.clone(),
        span: global.span.clone(),
    };
    let body = namespaces::render_namespace_body(module, &namespace, symbols, inferred, options)?;
    Ok(format!("declare global {{\n{body}}}\n"))
}

fn ambient_body(declarations: &[Declaration]) -> Vec<Declaration> {
    declarations
        .iter()
        .cloned()
        .map(|declaration| match declaration {
            Declaration::Namespace(mut namespace) => {
                namespace.declared = true;
                namespace.body = ambient_body(&namespace.body);
                Declaration::Namespace(namespace)
            }
            Declaration::Interface(mut interface) => {
                for field in &mut interface.fields {
                    field.value = multiline_record(&field.value, 4);
                }
                Declaration::Interface(interface)
            }
            other => other,
        })
        .collect()
}

fn multiline_record(value: &Type, indent: usize) -> Type {
    let Type::Record(fields) = value else {
        return value.clone();
    };
    let fields = fields
        .iter()
        .cloned()
        .map(|mut field| {
            field.value = multiline_record(&field.value, indent + 4);
            field
        })
        .collect::<Vec<_>>();
    Type::Literal(callable_objects::render_with_indent(
        &fields,
        &[],
        &[],
        indent,
    ))
}
