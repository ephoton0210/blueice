// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generic substitution and span-independent type identities.
use super::*;

/// Resolves omitted trailing generic arguments through their declaration-site
/// defaults. The parser/checker reports invalid argument counts and constraint
/// violations separately; this helper is also used by structural expansion,
/// where `None` simply means the named type cannot be expanded safely.
pub(crate) fn complete_type_arguments(
    parameters: &[TypeParameter],
    supplied: &[Type],
) -> Option<Vec<Type>> {
    if supplied.len() > parameters.len() {
        return None;
    }
    let mut substitutions = BTreeMap::new();
    let mut arguments = Vec::with_capacity(parameters.len());
    for (index, parameter) in parameters.iter().enumerate() {
        let value = supplied.get(index).cloned().or_else(|| {
            parameter
                .default
                .as_ref()
                .map(|value| substitute_type(value, &substitutions))
        })?;
        substitutions.insert(parameter.name.clone(), value.clone());
        substitutions.insert(
            crate::parser::type_parameter_identity(parameter),
            value.clone(),
        );
        arguments.push(value);
    }
    Some(arguments)
}

pub(crate) fn substitute_type(value: &Type, substitutions: &BTreeMap<String, Type>) -> Type {
    if let Type::InferenceBarrier(base) = value {
        return Type::inference_barrier(substitute_type(base, substitutions));
    }
    if let Type::Conditional(conditional) = value {
        if let Type::Named { name, arguments } = &conditional.check {
            if arguments.is_empty() {
                match substitutions.get(name) {
                    Some(Type::Never) => return Type::Never,
                    Some(Type::Union(options)) => {
                        return Type::Union(
                            options
                                .iter()
                                .map(|option| {
                                    let mut substitutions = substitutions.clone();
                                    substitutions.insert(name.clone(), option.clone());
                                    substitute_type(value, &substitutions)
                                })
                                .collect(),
                        );
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(mapped) = value.map_operator(|value, bound| {
        let mut substitutions = substitutions.clone();
        for name in bound {
            substitutions.remove(name);
        }
        substitute_type(value, &substitutions)
    }) {
        return mapped;
    }

    match value {
        Type::KeyOf(value) => Type::KeyOf(Box::new(substitute_type(value, substitutions))),
        Type::IndexedAccess {
            object,
            index,
            index_span,
        } => Type::IndexedAccess {
            object: Box::new(substitute_type(object, substitutions)),
            index: Box::new(substitute_type(index, substitutions)),
            index_span: index_span.clone(),
        },
        Type::Predicate(predicate) => {
            let mut predicate = predicate.clone();
            predicate.target = predicate
                .target
                .as_ref()
                .map(|target| Box::new(substitute_type(target, substitutions)));
            Type::Predicate(predicate)
        }
        Type::Named { name, arguments } if arguments.is_empty() => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| value.clone()),
        Type::Named { name, arguments } => Type::Named {
            name: match substitutions.get(name) {
                Some(Type::Named { name, arguments }) if arguments.is_empty() => name.clone(),
                _ => name.clone(),
            },
            arguments: arguments
                .iter()
                .map(|argument| substitute_type(argument, substitutions))
                .collect(),
        },
        Type::Array(value) => Type::Array(Box::new(substitute_type(value, substitutions))),
        Type::Tuple(values) => Type::Tuple(
            values
                .iter()
                .map(|value| crate::parser::TupleTypeElement {
                    annotation: substitute_type(&value.annotation, substitutions),
                    optional: value.optional,
                    label: value.label.clone(),
                    rest: value.rest,
                })
                .collect(),
        ),
        Type::CallableRecord { fields, signatures } => {
            let Type::Record(fields) =
                substitute_type(&Type::Record(fields.clone()), substitutions)
            else {
                unreachable!()
            };
            Type::CallableRecord {
                fields,
                signatures: signatures
                    .iter()
                    .map(|signature| {
                        signature.map_types(|value| substitute_type(value, substitutions))
                    })
                    .collect(),
            }
        }
        Type::Record(fields) => Type::Record(
            fields
                .iter()
                .map(|field| TypeField {
                    accessor_write_type: field
                        .accessor_write_type
                        .as_ref()
                        .map(|value| Box::new(substitute_type(value, substitutions))),
                    method: field.method,
                    name: field.name.clone(),
                    readonly: field.readonly,
                    optional: field.optional,
                    value: substitute_type(&field.value, substitutions),
                    span: field.span.clone(),
                })
                .collect(),
        ),
        Type::GenericFunction {
            type_parameters,
            parameters,
            result,
            span,
        } => {
            let mut substitutions = substitutions.clone();
            for parameter in type_parameters {
                substitutions.remove(&parameter.name);
            }
            let Type::Function { parameters, result } = substitute_type(
                &Type::Function {
                    parameters: parameters.clone(),
                    result: result.clone(),
                },
                &substitutions,
            ) else {
                unreachable!()
            };
            Type::GenericFunction {
                type_parameters: type_parameters
                    .iter()
                    .map(|parameter| TypeParameter {
                        constraint: parameter
                            .constraint
                            .as_ref()
                            .map(|value| substitute_type(value, &substitutions)),
                        default: parameter
                            .default
                            .as_ref()
                            .map(|value| substitute_type(value, &substitutions)),
                        ..parameter.clone()
                    })
                    .collect(),
                parameters,
                result,
                span: span.clone(),
            }
        }
        Type::Function { parameters, result } => Type::Function {
            parameters: substitute_parameters(parameters, substitutions),
            result: Box::new(substitute_type(result, substitutions)),
        },
        Type::Union(values) => Type::Union(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        Type::Intersection(values) => Type::Intersection(
            values
                .iter()
                .map(|value| substitute_type(value, substitutions))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn substitute_parameters(
    parameters: &[Parameter],
    substitutions: &BTreeMap<String, Type>,
) -> Vec<Parameter> {
    let mut result = Vec::new();
    for parameter in parameters {
        let annotation = parameter
            .annotation
            .as_ref()
            .map(|value| substitute_type(value, substitutions));
        // Instantiating a parameter pack creates ordinary labelled parameters.
        // An explicitly authored tuple-rest annotation retains its spelling.
        if parameter.rest && matches!(parameter.annotation, Some(Type::Named { .. })) {
            if let Some(Type::Tuple(elements)) = &annotation {
                result.extend(elements.iter().enumerate().map(|(index, element)| {
                    Parameter {
                        name: element
                            .label
                            .clone()
                            .unwrap_or_else(|| format!("{}_{index}", parameter.name)),
                        pattern: None,
                        rest: element.rest,
                        optional: element.optional,
                        annotation: Some(if element.optional {
                            union([element.annotation.clone(), Type::Undefined])
                        } else {
                            element.annotation.clone()
                        }),
                        default: None,
                        ..parameter.clone()
                    }
                }));
                continue;
            }
        }
        result.push(Parameter {
            annotation,
            ..parameter.clone()
        });
    }
    result
}

pub(crate) fn type_identity(value: &Type) -> String {
    if let Some(value) = value.string_value() {
        return format!("utf16:{:?}", value.units());
    }
    match value {
        Type::StringMapping(value) => format!(
            "string-mapping({:?},{:?},{},{},{})",
            value.operation,
            value.origin.module,
            value.origin.start,
            value.origin.end,
            type_identity(&value.operand)
        ),
        Type::Intrinsic(value) => format!("intrinsic:{value:?}"),
        Type::InferenceBarrier(base) => format!("inference-barrier({})", type_identity(base)),
        Type::IndexedRecord { indices, .. } => format!(
            "{};mapped-domains:[{}]",
            value.operator_text(type_identity).expect("indexed record"),
            indices
                .iter()
                .map(|index| index
                    .mapped_domain
                    .as_ref()
                    .map(type_identity)
                    .unwrap_or_default())
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::UniqueSymbol(origin) => format!(
            "unique symbol {}:{}:{}",
            origin.module, origin.start, origin.end
        ),
        Type::TemplateLiteral(value) => {
            let chunk = |raw: &str| {
                crate::utf16::Utf16String::parse_template_chunk(raw)
                    .map(|value| format!("{:?}", value.units()))
                    .unwrap_or_else(|| format!("raw:{raw:?}"))
            };
            format!(
                "template({};{})",
                chunk(&value.head),
                value
                    .spans
                    .iter()
                    .map(|(value, tail)| format!("{}:{}", type_identity(value), chunk(tail)))
                    .collect::<Vec<_>>()
                    .join(";")
            )
        }
        Type::Conditional(_) | Type::Infer(_) | Type::Mapped(_) | Type::Readonly(_) => {
            value.operator_text(type_identity).expect("operator type")
        }
        Type::KeyOf(value) => format!("keyof({})", type_identity(value)),
        Type::IndexedAccess { object, index, .. } => {
            format!("{}[{}]", type_identity(object), type_identity(index))
        }
        Type::Predicate(predicate) => predicate.text(type_identity),
        Type::Named { name, arguments } => format!(
            "{name}<{}>",
            arguments
                .iter()
                .map(type_identity)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Array(value) => value.array_element_text(type_identity),
        Type::Tuple(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| {
                    format!(
                        "{}{:?}:{}{}",
                        if value.rest { "..." } else { "" },
                        value.label,
                        type_identity(&value.annotation),
                        if value.optional { "?" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Record(_)
        | Type::CallableRecord { .. }
        | Type::Function { .. }
        | Type::GenericFunction { .. } => super::identity_shapes::render(value, type_identity),
        Type::Union(values) => values
            .iter()
            .map(type_identity)
            .collect::<Vec<_>>()
            .join("|"),
        Type::Intersection(values) => values
            .iter()
            .map(type_identity)
            .collect::<Vec<_>>()
            .join("&"),
        _ => type_label(value),
    }
}

pub(crate) fn type_label(value: &Type) -> String {
    match value {
        Type::Conditional(_)
        | Type::StringMapping(_)
        | Type::InferenceBarrier(_)
        | Type::Infer(_)
        | Type::Mapped(_)
        | Type::TemplateLiteral(_)
        | Type::Readonly(_)
        | Type::IndexedRecord { .. } => value.operator_text(type_label).expect("operator type"),
        Type::KeyOf(value) => {
            let text = type_label(value);
            if value.needs_parentheses_after_keyof() {
                format!("keyof ({text})")
            } else {
                format!("keyof {text}")
            }
        }
        Type::IndexedAccess { object, index, .. } => {
            format!("{}[{}]", type_label(object), type_label(index))
        }
        Type::Predicate(predicate) => predicate.text(type_label),
        Type::Any => "any".to_string(),
        Type::Unknown | Type::StrictUnknown => "unknown".to_string(),
        Type::BigInt => "bigint".to_string(),
        Type::Symbol => "symbol".to_string(),
        Type::UniqueSymbol(_) => "unique symbol".to_string(),
        Type::ConstAssertion => "const".to_string(),
        Type::Never => "never".to_string(),
        Type::Void => "void".to_string(),
        Type::Null => "null".to_string(),
        Type::Undefined => "undefined".to_string(),
        Type::Boolean => "boolean".to_string(),
        Type::Number => "number".to_string(),
        Type::String => "string".to_string(),
        Type::Literal(value) => value.clone(),
        Type::StringLiteral(value) => value.diagnostic_text(),
        Type::Intrinsic(_) => "intrinsic".to_string(),
        Type::Named { name, .. } => crate::parser::source_type_name(name).to_string(),
        Type::Array(value) => value.array_element_text(type_label),
        Type::Tuple(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| {
                    format!(
                        "{}{}{}",
                        if value.rest { "..." } else { "" },
                        type_label(&value.annotation),
                        if value.optional { "?" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Record(_) | Type::CallableRecord { .. } => "record".to_string(),
        Type::Function { .. } | Type::GenericFunction { .. } => "function".to_string(),
        Type::Union(values) => values
            .iter()
            .map(type_label)
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Intersection(values) => values
            .iter()
            .map(type_label)
            .collect::<Vec<_>>()
            .join(" & "),
    }
}
