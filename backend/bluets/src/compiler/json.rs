// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authorized JSON keeps its original bytes and derives only a static schema.

use super::*;
use crate::parser::TypeField;
use crate::Type;
use serde_json::Value;

pub(super) fn parse(
    source: &ModuleSource,
    limits: &CompilerLimits,
) -> Result<(Module, Type), Diagnostic> {
    let span = SourceSpan::new(&source.id, 0, source.text.len());
    crate::syntax::lex_with_limits(
        &source.id,
        &source.text,
        limits.parser.max_source_bytes,
        limits.parser.max_tokens,
    )
    .map_err(|diagnostics| {
        diagnostics
            .into_iter()
            .next()
            .expect("lexer errors retain their diagnostic")
    })?;
    let value: Value = serde_json::from_str(&source.text).map_err(|error| {
        Diagnostic::error(
            DiagnosticCode::ParseError,
            span.clone(),
            format!("invalid JSON source: {error}"),
        )
    })?;
    let schema = schema(&value, &span, limits.parser.max_type_depth)?;
    // Data has no executable declarations. The raw source remains available
    // for diagnostics, content fingerprints and exact asset publication.
    let mut module = parse_module(&source.id, String::new()).expect("empty static data module");
    module.source = source.text.clone();
    Ok((module, schema))
}

fn schema(value: &Value, span: &SourceSpan, depth: usize) -> Result<Type, Diagnostic> {
    if depth == 0 {
        return Err(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            span.clone(),
            "JSON schema exceeds the type-depth limit",
        ));
    }
    Ok(match value {
        Value::Null => Type::Null,
        Value::Bool(_) => Type::Boolean,
        Value::Number(_) => Type::Number,
        Value::String(_) => Type::String,
        Value::Array(values) => {
            let mut items = Vec::new();
            for value in values {
                let value = schema(value, span, depth - 1)?;
                if !items.contains(&value) {
                    items.push(value);
                }
            }
            let item = match items.len() {
                0 => Type::Never,
                1 => items.pop().unwrap(),
                _ => Type::Union(items),
            };
            Type::Array(Box::new(item))
        }
        Value::Object(values) => Type::Record(
            values
                .iter()
                .map(|(name, value)| {
                    Ok(TypeField {
                        name: name.clone(),
                        value: schema(value, span, depth - 1)?,
                        span: span.clone(),
                        readonly: false,
                        optional: false,
                        accessor_write_type: None,
                        method: false,
                    })
                })
                .collect::<Result<_, Diagnostic>>()?,
        ),
    })
}
