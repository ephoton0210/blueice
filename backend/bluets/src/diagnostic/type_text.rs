// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Diagnostic type spelling is separate from stable BTS prose and checking.
use crate::parser::{Parameter, Type};
mod names;
mod parameters;
mod relations;
mod signatures;
mod tuples;
pub(crate) use parameters::list as parameter_list_in;
pub(crate) use relations::detail;

pub(crate) fn render(value: &Type) -> String {
    text(value, 0, None)
}
pub(crate) fn render_in(value: &Type, project: &crate::Project) -> String {
    text(value, 0, Some(project))
}

fn text(value: &Type, depth: usize, project: Option<&crate::Project>) -> String {
    if depth >= 128 {
        return "...".into();
    }
    let render = |value: &Type| text(value, depth + 1, project);
    match value {
        Type::Conditional(_) | Type::Infer(_) | Type::Mapped(_) | Type::TemplateLiteral(_) => {
            value.operator_text(render).expect("operator type")
        }
        Type::KeyOf(value) => format!("keyof {}", render(value)),
        Type::IndexedAccess { object, index, .. } => {
            format!("{}[{}]", render(object), render(index))
        }
        Type::Predicate(predicate) => predicate.text(render),
        Type::Literal(value) => {
            if value.starts_with('\'') && value.ends_with('\'') {
                serde_json::to_string(&value[1..value.len() - 1]).unwrap()
            } else {
                value.clone()
            }
        }
        Type::Named { name, arguments } => {
            if name == "ReadonlyArray" && arguments.len() == 1 {
                return format!("readonly {}", arguments[0].array_element_text(render));
            }
            let name = names::display(name, project);
            if arguments.is_empty() {
                name
            } else {
                format!(
                    "{name}<{}>",
                    arguments.iter().map(render).collect::<Vec<_>>().join(", ")
                )
            }
        }
        Type::Array(value) => value.array_element_text(render),
        Type::Tuple(values) => format!(
            "[{}]",
            values
                .iter()
                .map(|value| {
                    let label = value
                        .label
                        .as_ref()
                        .map(|name| format!("{name}{}: ", if value.optional { "?" } else { "" }))
                        .unwrap_or_default();
                    format!(
                        "{}{label}{}{}",
                        if value.rest { "..." } else { "" },
                        if value.optional
                            && value.label.is_none()
                            && matches!(value.annotation, Type::Union(_))
                        {
                            format!("({})", render(&value.annotation))
                        } else {
                            render(&value.annotation)
                        },
                        if value.optional && value.label.is_none() {
                            "?"
                        } else {
                            ""
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::CallableRecord { fields, signatures } => {
            let mut members = fields
                .iter()
                .map(|field| {
                    format!(
                        "{}{}{}: {};",
                        if field.readonly { "readonly " } else { "" },
                        field.name,
                        if field.optional { "?" } else { "" },
                        render(&field.value)
                    )
                })
                .collect::<Vec<_>>();
            members.extend(signatures.iter().map(|signature| {
                format!(
                    "{}({}): {};",
                    if signature.construct { "new " } else { "" },
                    parameter_list_in(&signature.parameters, project),
                    render(&signature.result)
                )
            }));
            format!("{{ {} }}", members.join(" "))
        }
        Type::Record(fields) => {
            if let Some(project) = project {
                if let Some(prototype) = fields
                    .iter()
                    .find(|field| field.name == "prototype" && field.readonly)
                {
                    fn class_at<'a>(
                        declarations: &'a [crate::Declaration],
                        span: &crate::SourceSpan,
                    ) -> Option<&'a str> {
                        declarations
                            .iter()
                            .find_map(|declaration| match declaration {
                                crate::Declaration::Class(class) if class.name_span == *span => {
                                    Some(class.name.as_str())
                                }
                                crate::Declaration::Namespace(ns) => class_at(&ns.body, span),
                                _ => None,
                            })
                    }
                    if let Some(name) = project
                        .modules
                        .values()
                        .find_map(|module| class_at(&module.declarations, &prototype.span))
                    {
                        return format!("typeof {name}");
                    }
                }
            }
            if fields.is_empty() {
                return "{}".into();
            }
            let fields = fields.iter().map(|field| {
                let optional=field.optional && !matches!(field.value,Type::Any|Type::Unknown);
                let mut value = if optional && !matches!(&field.value,Type::Union(values) if values.contains(&Type::Undefined)) {
                    let value=render(&field.value);
                    if matches!(field.value,Type::Function {..}) {format!("({value}) | undefined")} else {format!("{value} | undefined")}
                } else { render(&field.value) };
                if let Type::Function { parameters,result } = &field.value {
                    // A method signature keeps its original method syntax.
                    let source_method = project.and_then(|project|project.source(&field.span.module).or_else(||(field.span.module=="<inferred>").then(||project.modules.values().find_map(|module|module.source.get(field.span.start..field.span.end).filter(|source|source.trim_start().starts_with(&field.name)).map(|_|module.source.as_str()))).flatten())).and_then(|source|source.get(field.span.start..field.span.end)).is_some_and(|source|source.trim_start().strip_prefix(&field.name).is_some_and(|rest|rest.trim_start_matches('?').trim_start().starts_with('(')));
                    if source_method { return format!("{}{}({}): {};",field.name,if field.optional {"?"} else {""},parameters::list(parameters,project),render(result)); }
                }
                if optional && matches!(field.value,Type::Union(_)) {value=render(&Type::Union(match &field.value {Type::Union(values)=>{let mut values=values.clone();if !values.contains(&Type::Undefined) {values.push(Type::Undefined)} values},_=>unreachable!()}));}
                format!("{}{}{}: {value};",if field.readonly {"readonly "} else {""},field.name,if field.optional {"?"} else {""})
            }).collect::<Vec<_>>().join(" ");
            format!("{{ {fields} }}")
        }
        Type::GenericFunction {
            type_parameters,
            parameters,
            result,
            ..
        } => {
            let generic = type_parameters
                .iter()
                .map(|p| {
                    let mut text = p.name.clone();
                    if let Some(constraint) = &p.constraint {
                        text.push_str(&format!(" extends {}", render(constraint)));
                    }
                    if let Some(default) = &p.default {
                        text.push_str(&format!(" = {}", render(default)));
                    }
                    text
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "<{generic}>({}) => {}",
                parameters::list(parameters, project),
                render(result)
            )
        }
        Type::Function { parameters, result } => {
            format!(
                "({}) => {}",
                parameters::list(parameters, project),
                render(result)
            )
        }
        Type::Union(values) => {
            let literal_union = values.iter().all(|value| matches!(value, Type::Literal(_)));
            let mut values = values
                .iter()
                .map(|value| {
                    if matches!(value, Type::Function { .. }) {
                        format!("({})", render(value))
                    } else {
                        render(value)
                    }
                })
                .collect::<Vec<_>>();
            if !literal_union {
                values.sort_by_key(|value| {
                    (
                        match value.as_str() {
                            "string" => 0,
                            "number" => 1,
                            "bigint" => 2,
                            "boolean" => 3,
                            "null" => 10,
                            "undefined" => 11,
                            _ => 5,
                        },
                        value.clone(),
                    )
                });
            }
            values.dedup();
            values.join(" | ")
        }
        Type::Intersection(values) => values.iter().map(render).collect::<Vec<_>>().join(" & "),
        Type::Any => "any".into(),
        Type::Unknown => "unknown".into(),
        Type::Never => "never".into(),
        Type::Void => "void".into(),
        Type::Null => "null".into(),
        Type::Undefined => "undefined".into(),
        Type::Boolean => "boolean".into(),
        Type::Number => "number".into(),
        Type::String => "string".into(),
    }
}

pub(crate) fn argument(actual: &Type, expected: &Type) -> String {
    match (actual, expected) {
        (Type::Literal(value), Type::Union(values))
            if !values.iter().any(|value| matches!(value, Type::Literal(_))) =>
        {
            if value.starts_with(['\'', '"']) {
                "string".into()
            } else if value.parse::<f64>().is_ok() {
                "number".into()
            } else {
                render(actual)
            }
        }
        (
            Type::Literal(value),
            Type::Number
            | Type::String
            | Type::Boolean
            | Type::Array(_)
            | Type::Record(_)
            | Type::Function { .. },
        ) => {
            if value.starts_with(['\'', '"']) {
                "string".into()
            } else if value == "true" || value == "false" {
                "boolean".into()
            } else {
                "number".into()
            }
        }
        _ => render(actual),
    }
}

pub(crate) fn argument_in(actual: &Type, expected: &Type, project: &crate::Project) -> String {
    let value = argument(actual, expected);
    if value == render(actual) {
        render_in(actual, project)
    } else {
        value
    }
}
pub(crate) fn default_parameter(value: &Type, parameter: &Parameter) -> Type {
    if parameter.default.is_none() {
        return value.clone();
    }
    if let Type::Union(values) = value {
        let mut values = values
            .iter()
            .filter(|value| **value != Type::Undefined)
            .cloned()
            .collect::<Vec<_>>();
        if values.len() == 1 {
            values.pop().unwrap()
        } else {
            Type::Union(values)
        }
    } else {
        value.clone()
    }
}
