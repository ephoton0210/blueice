// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Anonymous constructor declarations expose structural instances, not private identities.

use super::*;
use crate::checker::{complete_type_arguments, substitute_type, ClassExpressionSurface};
use crate::parser::{Parameter, TypeField, TypeSignature};
use std::collections::BTreeSet;

mod binders;
pub(super) use binders::rename_shadowed_binders;

pub(super) fn declaration_type(
    value: &Type,
    surfaces: &BTreeMap<String, ClassExpressionSurface>,
    span: &SourceSpan,
) -> Result<Type, Diagnostic> {
    if surfaces.is_empty() {
        return Ok(value.clone());
    }
    let mut visiting = surfaces
        .iter()
        .filter(|(_, surface)| &surface.constructor == value)
        .map(|(name, _)| format!("typeof {name}"))
        .collect();
    expand(value, surfaces, span, &mut visiting, &mut 8192)
}

fn expand(
    value: &Type,
    surfaces: &BTreeMap<String, ClassExpressionSurface>,
    span: &SourceSpan,
    visiting: &mut BTreeSet<String>,
    fuel: &mut usize,
) -> Result<Type, Diagnostic> {
    if *fuel == 0 {
        return Err(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            span.clone(),
            "class expression declaration expansion exceeds its node limit",
        ));
    }
    *fuel -= 1;
    let reference = match value {
        Type::Named { name, arguments } => Some((name, arguments.as_slice())),
        Type::Literal(name) if name.starts_with("typeof ") => Some((name, &[][..])),
        _ => None,
    };
    if let Some((name, arguments)) = reference {
        let spelling = if name.starts_with("typeof ")
            && !surfaces.contains_key(name.trim_start_matches("typeof "))
        {
            crate::parser::source_type_name(name)
        } else {
            name
        };
        let identity = spelling.strip_prefix("typeof ").unwrap_or(spelling);
        if let Some(surface) = surfaces.get(identity) {
            if !visiting.insert(spelling.to_string()) {
                return Ok(Type::Literal("/*elided*/ any".into()));
            }
            let substitutions = complete_type_arguments(&surface.parameters, arguments)
                .unwrap_or_else(|| vec![Type::Any; surface.parameters.len()])
                .into_iter()
                .zip(&surface.parameters)
                .map(|(argument, parameter)| (parameter.name.clone(), argument))
                .collect();
            let instance = if name.starts_with("typeof ") {
                surface.constructor.clone()
            } else {
                substitute_type(&surface.instance, &substitutions)
            };
            let result = expand(&instance, surfaces, span, visiting, fuel);
            visiting.remove(spelling);
            return result;
        }
    }
    let mut failure = None;
    let mut child = |value: &Type| match expand(value, surfaces, span, visiting, fuel) {
        Ok(value) => value,
        Err(error) => {
            failure = Some(error);
            Type::Unknown
        }
    };
    if let Some(mapped) = value.map_operator(|value, _| child(value)) {
        return failure.map_or(Ok(mapped), Err);
    }
    let result = match value {
        Type::Named { name, arguments } => Type::Named {
            name: name.clone(),
            arguments: arguments.iter().map(&mut child).collect(),
        },
        Type::Record(fields) => Type::Record(fields.iter().map(|field| TypeField {
            accessor_write_type: field.accessor_write_type.as_ref().map(|value| Box::new(child(value))),
                    value: child(&field.value), ..field.clone()
        }).collect()),
        Type::CallableRecord { fields, signatures } => Type::CallableRecord {
            fields: fields.iter().filter(|field| {
                !(field.name == "prototype" && matches!(&field.value, Type::Named { name, .. } if surfaces.contains_key(name)))
            }).map(|field| TypeField { accessor_write_type: field.accessor_write_type.as_ref().map(|value| Box::new(child(value))),
                    value: child(&field.value), ..field.clone() }).collect(),
            signatures: signatures.iter().map(|signature| TypeSignature {
                type_parameters: signature.type_parameters.iter().map(|parameter| TypeParameter {
                    constraint: parameter.constraint.as_ref().map(&mut child),
                    default: parameter.default.as_ref().map(&mut child),
                    ..parameter.clone()
                }).collect(),
                parameters: signature.parameters.iter().map(|parameter| Parameter {
                    annotation: parameter.annotation.as_ref().map(&mut child),
                    ..parameter.clone()
                }).collect(),
                result: child(&signature.result),
                ..signature.clone()
            }).collect(),
        },
        Type::Function { parameters, result } => Type::Function {
            parameters: parameters.iter().map(|parameter| Parameter {
                annotation: parameter.annotation.as_ref().map(&mut child), ..parameter.clone()
            }).collect(),
            result: Box::new(child(result)),
        },
        Type::GenericFunction { type_parameters, parameters, result, span } => Type::GenericFunction {
            type_parameters: type_parameters.iter().map(|parameter| TypeParameter {
                constraint: parameter.constraint.as_ref().map(&mut child),
                default: parameter.default.as_ref().map(&mut child),
                ..parameter.clone()
            }).collect(),
            parameters: parameters.iter().map(|parameter| Parameter {
                annotation: parameter.annotation.as_ref().map(&mut child), ..parameter.clone()
            }).collect(),
            result: Box::new(child(result)),
            span: span.clone(),
        },
        Type::Array(value) => Type::Array(Box::new(child(value))),
        Type::Tuple(elements) => Type::Tuple(elements.iter().map(|element| crate::parser::TupleTypeElement {
            annotation: child(&element.annotation), ..element.clone()
        }).collect()),
        Type::Union(options) => Type::Union(options.iter().map(&mut child).collect()),
        Type::Intersection(options) => Type::Intersection(options.iter().map(&mut child).collect()),
        other => other.clone(),
    };
    failure.map_or(Ok(result), Err)
}
