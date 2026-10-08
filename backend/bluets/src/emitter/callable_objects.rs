// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration object members preserve their original signature/data order.

use super::*;

pub(super) fn render(
    fields: &[crate::parser::TypeField],
    signatures: &[crate::parser::TypeSignature],
) -> String {
    if fields.is_empty() && signatures.len() == 1 && signatures[0].constructor_arrow {
        let signature = &signatures[0];
        return format!(
            "{}new {}",
            if signature.abstract_constructor {
                "abstract "
            } else {
                ""
            },
            type_to_ts(&signature.function_type())
        );
    }
    render_with_indices(fields, signatures, &[])
}

pub(super) fn render_with_indices(
    fields: &[crate::parser::TypeField],
    signatures: &[crate::parser::TypeSignature],
    indices: &[crate::parser::IndexSignature],
) -> String {
    render_with_indent(fields, signatures, indices, 0)
}

pub(super) fn render_with_indent(
    fields: &[crate::parser::TypeField],
    signatures: &[crate::parser::TypeSignature],
    indices: &[crate::parser::IndexSignature],
    indent: usize,
) -> String {
    let mut members = Vec::new();
    for index in indices {
        members.push((
            index.span.start,
            format!(
                "{}[{}: {}]: {}",
                if index.readonly { "readonly " } else { "" },
                index.name,
                type_to_ts(&index.key),
                type_to_ts(&index.value)
            ),
        ));
    }
    for field in fields {
        let name = format!(
            "{}{}{}",
            if field.readonly { "readonly " } else { "" },
            property_name(&field.name),
            if field.optional { "?" } else { "" }
        );
        let text = match &field.value {
            Type::Function { parameters, result } if field.method => {
                format!("{name}{}", method_signature_to_ts(parameters, result))
            }
            Type::GenericFunction {
                type_parameters,
                parameters,
                result,
                ..
            } if field.method => {
                let mut text = name;
                emit_type_parameters(&mut text, type_parameters);
                text.push_str(&method_signature_to_ts(parameters, result));
                text
            }
            _ => format!("{name}: {}", type_to_ts(&field.value)),
        };
        members.push((field.span.start, text));
    }
    for signature in signatures {
        let mut text = if signature.construct {
            "new ".to_string()
        } else {
            String::new()
        };
        emit_type_parameters(&mut text, &signature.type_parameters);
        text.push_str(&method_signature_to_ts(
            &signature.parameters,
            &signature.result,
        ));
        members.push((signature.span.start, text));
    }
    members.sort_by_key(|(start, _)| *start);
    if members.is_empty() {
        return "{}".into();
    }
    format!(
        "{{\n{}\n{}}}",
        members
            .into_iter()
            .map(|(_, text)| format!("{}{text};", " ".repeat(indent + 4)))
            .collect::<Vec<_>>()
            .join("\n"),
        " ".repeat(indent)
    )
}

fn property_name(name: &str) -> String {
    let mut characters = name.chars();
    let identifier = characters
        .next()
        .is_some_and(|c| c.is_alphabetic() || matches!(c, '_' | '$'))
        && characters.all(|c| c.is_alphanumeric() || matches!(c, '_' | '$'));
    if identifier || name.parse::<f64>().is_ok() || name.starts_with(['[', '\'', '"']) {
        name.into()
    } else {
        serde_json::to_string(name).expect("a property name is a JSON string")
    }
}
