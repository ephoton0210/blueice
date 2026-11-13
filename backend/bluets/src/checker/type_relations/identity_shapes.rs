// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structural cache identities recurse into values before any UTF-8 printing.

use super::*;
use crate::parser::{Parameter, TypeField, TypeParameter, TypeSignature};

type Render = fn(&Type) -> String;

fn fields(values: &[TypeField], render: Render) -> String {
    format!(
        "{:?}",
        values
            .iter()
            .map(|field| (
                &field.name,
                field.readonly,
                field.optional,
                field.method,
                render(&field.value),
                field.accessor_write_type.as_deref().map(render),
            ))
            .collect::<Vec<_>>()
    )
}

fn parameters(values: &[Parameter], render: Render) -> String {
    format!(
        "{:?}",
        values
            .iter()
            .map(|parameter| (
                &parameter.name,
                parameter.rest,
                parameter.optional,
                parameter.default.is_some(),
                parameter.annotation.as_ref().map(render),
            ))
            .collect::<Vec<_>>()
    )
}

fn binders(values: &[TypeParameter], render: Render) -> String {
    format!(
        "{:?}",
        values
            .iter()
            .map(|parameter| (
                &parameter.name,
                parameter.is_const,
                parameter.variance,
                parameter.constraint.as_ref().map(render),
                parameter.default.as_ref().map(render),
            ))
            .collect::<Vec<_>>()
    )
}

fn signature(value: &TypeSignature, render: Render) -> String {
    format!(
        "signature({},{},{:?},{},{},{},{})",
        value.construct,
        value.abstract_constructor,
        value.constructor_visibility,
        value.constructor_arrow,
        binders(&value.type_parameters, render),
        parameters(&value.parameters, render),
        render(&value.result)
    )
}

pub(super) fn render(value: &Type, render: Render) -> String {
    match value {
        Type::Record(values) => format!("record({})", fields(values, render)),
        Type::CallableRecord {
            fields: values,
            signatures,
        } => format!(
            "callable({},{:?})",
            fields(values, render),
            signatures
                .iter()
                .map(|value| signature(value, render))
                .collect::<Vec<_>>()
        ),
        Type::Function {
            parameters: values,
            result,
        } => format!(
            "function({},{})",
            parameters(values, render),
            render(result)
        ),
        Type::GenericFunction {
            type_parameters,
            parameters: values,
            result,
            ..
        } => format!(
            "generic({},{},{})",
            binders(type_parameters, render),
            parameters(values, render),
            render(result)
        ),
        _ => unreachable!("structural identity requires a structural type"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceSpan, StringLiteral, Utf16String};

    fn record(units: &[u16], start: usize) -> Type {
        Type::Record(vec![TypeField {
            name: "value".into(),
            readonly: false,
            optional: false,
            method: false,
            accessor_write_type: None,
            span: SourceSpan::new("owner", start, start + 1),
            value: Type::StringLiteral(StringLiteral::generated(Utf16String::from_units(
                units.to_vec(),
            ))),
        }])
    }

    #[test]
    fn records_and_functions_keep_semantic_units_when_diagnostic_tokens_coincide() {
        let high = record(&[0xd800], 0);
        let replacement = record(&[0xfffd], 0);
        assert_eq!(
            crate::diagnostic::type_text::render(&high),
            crate::diagnostic::type_text::render(&replacement)
        );
        assert_ne!(type_identity(&high), type_identity(&replacement));
        let function = |result| Type::Function {
            parameters: Vec::new(),
            result: Box::new(result),
        };
        assert_ne!(
            type_identity(&function(high.clone())),
            type_identity(&function(replacement))
        );
        assert_eq!(type_identity(&high), type_identity(&record(&[0xd800], 42)));
    }
}
