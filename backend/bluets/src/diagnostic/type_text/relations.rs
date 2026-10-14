// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain an already-rejected type pair; this never selects a checker verdict.
use super::*;
use crate::parser::{Declaration, Visibility};

pub(crate) fn detail(actual: &Type, expected: &Type, project: &crate::Project) -> String {
    reason(actual, expected, project, 1)
}
pub(super) fn line(depth: usize, message: String) -> String {
    format!("\n{}{message}", "  ".repeat(depth))
}
pub(super) fn mismatch(
    actual: &Type,
    expected: &Type,
    project: &crate::Project,
    depth: usize,
) -> String {
    let text = line(
        depth,
        format!(
            "Type '{}' is not assignable to type '{}'.",
            render_in(actual, project),
            render_in(expected, project)
        ),
    );
    text + &reason(actual, expected, project, depth + 1)
}
pub(super) fn reason(
    actual: &Type,
    expected: &Type,
    project: &crate::Project,
    depth: usize,
) -> String {
    if depth > 16 || actual == expected {
        return String::new();
    }
    if let Type::Union(values) = actual {
        if let Some(value) = values
            .iter()
            .find(|value| **value == Type::Undefined)
            .filter(|_| !contains(expected, &Type::Undefined))
        {
            return mismatch(value, expected, project, depth);
        }
        if let Some(value) = values.iter().find(|value| !contains(expected, value)) {
            let target = if let Type::Union(values) = expected {
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
                expected.clone()
            };
            return mismatch(value, &target, project, depth);
        }
    }
    if let (Type::Array(actual), Type::Array(expected)) = (actual, expected) {
        return mismatch(actual, expected, project, depth);
    }
    if let Some(detail) = tuples::reason(actual, expected, project, depth) {
        return detail;
    }
    if let (
        Type::Named {
            name: actual_name,
            arguments: actual_args,
        },
        Type::Named {
            name: expected_name,
            arguments: expected_args,
        },
    ) = (actual, expected)
    {
        if actual_name == expected_name {
            if actual_name == "Promise" && actual_args.len() == 1 && expected_args.len() == 1 {
                return mismatch(&actual_args[0], &expected_args[0], project, depth);
            }
            if matches!(actual_name.as_str(), "Generator" | "Iterable")
                && !actual_args.is_empty()
                && !expected_args.is_empty()
                && actual_args.first() != expected_args.first()
            {
                let actual_return = actual_args.get(1).unwrap_or(&Type::Any);
                let expected_return = expected_args.get(1).unwrap_or(&Type::Any);
                return iterator_reason(
                    (&actual_args[0], actual_return),
                    (&expected_args[0], expected_return),
                    project,
                    depth,
                    actual_name == "Iterable",
                    false,
                );
            }
            if actual_name == "Generator"
                && actual_args.get(1) != expected_args.get(1)
                && actual_args.len() > 1
                && expected_args.len() > 1
            {
                return iterator_reason(
                    (&actual_args[0], &actual_args[1]),
                    (&expected_args[0], &expected_args[1]),
                    project,
                    depth,
                    false,
                    true,
                );
            }
            if let Some((actual, expected)) = actual_args
                .iter()
                .zip(expected_args)
                .find(|(actual, expected)| actual != expected)
            {
                return mismatch(actual, expected, project, depth);
            }
        }
    }
    if let (
        Type::Function {
            parameters: actual_params,
            result: actual_result,
        },
        Type::Function {
            parameters: expected_params,
            result: expected_result,
        },
    ) = (actual, expected)
    {
        let detail = signatures::reason(actual_params, expected_params, project, depth);
        if !detail.is_empty() {
            return detail;
        }
        if let Type::Predicate(target) = expected_result.as_ref() {
            if let Type::Predicate(source) = actual_result.as_ref() {
                let actual_index = actual_params
                    .iter()
                    .position(|p| p.name == source.parameter);
                let expected_index = expected_params
                    .iter()
                    .position(|p| p.name == target.parameter);
                if actual_index != expected_index {
                    return line(
                        depth,
                        format!(
                            "Type predicate '{}' is not assignable to '{}'.",
                            render_in(actual_result, project),
                            render_in(expected_result, project)
                        ),
                    ) + &line(
                        depth + 1,
                        format!(
                            "Parameter '{}' is not in the same position as parameter '{}'.",
                            source.parameter, target.parameter
                        ),
                    );
                }
            } else if !target.asserts {
                return line(
                    depth,
                    format!(
                        "Signature '({}): {}' must be a type predicate.",
                        parameters::list(actual_params, Some(project)),
                        render_in(actual_result, project)
                    ),
                );
            }
        }
        if **expected_result != Type::Void
            && **expected_result != Type::Any
            && actual_result != expected_result
        {
            return mismatch(actual_result, expected_result, project, depth);
        }
    }
    if let Some(text) = nominal(actual, expected, project, depth) {
        return text;
    }
    if let (Type::Record(actual), Type::Record(expected)) = (actual, expected) {
        for source in actual.iter().filter(|field| field.optional) {
            if let Some(target) = expected
                .iter()
                .find(|field| field.name == source.name && !field.optional)
            {
                let value = Type::Union(vec![source.value.clone(), Type::Undefined]);
                if contains(&target.value, &Type::Undefined) {
                    return line(
                        depth,
                        format!(
                            "Property '{}' is optional in type '{}' but required in type '{}'.",
                            source.name,
                            render_in(&Type::Record(actual.clone()), project),
                            render_in(&Type::Record(expected.clone()), project)
                        ),
                    );
                }
                return line(
                    depth,
                    format!("Types of property '{}' are incompatible.", source.name),
                ) + &mismatch(&value, &target.value, project, depth + 1);
            }
        }
        if let Some((actual, expected)) = actual.iter().find_map(|actual| {
            expected
                .iter()
                .find(|expected| expected.name == actual.name && expected.value != actual.value)
                .map(|expected| (actual, expected))
        }) {
            return line(
                depth,
                format!("Types of property '{}' are incompatible.", actual.name),
            ) + &mismatch(&actual.value, &expected.value, project, depth + 1);
        }
    }
    String::new()
}
pub(super) fn contains(expected: &Type, actual: &Type) -> bool {
    expected == actual
        || matches!(expected, Type::Any | Type::Unknown)
        || match expected {
            Type::Union(values) => values.iter().any(|value| contains(value, actual)),
            Type::String => matches!(actual,Type::Literal(value) if value.starts_with(['\'','"'])),
            Type::Number => matches!(actual,Type::Literal(value) if value.parse::<f64>().is_ok()),
            _ => false,
        }
}
fn iterator_reason(
    (actual, actual_return): (&Type, &Type),
    (expected, expected_return): (&Type, &Type),
    project: &crate::Project,
    depth: usize,
    iterable: bool,
    returns: bool,
) -> String {
    let a = render_in(actual, project);
    let ar = render_in(actual_return, project);
    let e = render_in(expected, project);
    let er = render_in(expected_return, project);
    let member = if iterable {
        "[Symbol.iterator]().next(...)"
    } else {
        "next(...)"
    };
    let variant = if returns {
        "IteratorReturnResult"
    } else {
        "IteratorYieldResult"
    };
    let (av, ev) = if returns {
        (ar.as_str(), er.as_str())
    } else {
        (a.as_str(), e.as_str())
    };
    line(depth,format!("The types returned by '{member}' are incompatible between these types."))
        +&line(depth+1,format!("Type 'IteratorResult<{a}, {ar}>' is not assignable to type 'IteratorResult<{e}, {er}>'."))
        +&line(depth+2,format!("Type '{variant}<{av}>' is not assignable to type 'IteratorResult<{e}, {er}>'."))
        +&line(depth+3,format!("Type '{variant}<{av}>' is not assignable to type '{variant}<{ev}>'."))
        +&mismatch(if returns {actual_return} else {actual},if returns {expected_return} else {expected},project,depth+4)
}
fn nominal(
    actual: &Type,
    expected: &Type,
    project: &crate::Project,
    depth: usize,
) -> Option<String> {
    let a = render_in(actual, project);
    let e = render_in(expected, project);
    let mut restricted = Vec::new();
    fn collect<'a>(
        items: &'a [Declaration],
        names: &[&str],
        result: &mut Vec<(&'a crate::ClassDeclaration, String, Visibility)>,
    ) {
        for item in items {
            match item {
                Declaration::Namespace(ns) => collect(&ns.body, names, result),
                Declaration::Class(class) if names.contains(&class.name.as_str()) => {
                    for field in class.parameter_property_fields() {
                        if field.visibility != Visibility::Public {
                            result.push((class, field.name, field.visibility));
                        }
                    }
                    for member in &class.members {
                        let visibility = member
                            .field
                            .as_ref()
                            .map(|field| field.visibility)
                            .or_else(|| member.method.as_ref().map(|method| method.visibility))
                            .or_else(|| {
                                member.accessor.as_ref().map(|accessor| accessor.visibility)
                            })
                            .unwrap_or(Visibility::Public);
                        if let Some(name) = &member.name {
                            if visibility != Visibility::Public || name.starts_with('#') {
                                result.push((class, name.clone(), visibility));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for module in project.modules.values() {
        collect(&module.declarations, &[&a, &e], &mut restricted);
    }
    let &(class, ref name, visibility) = restricted.first()?;
    let owner = &class.name;
    if restricted
        .iter()
        .any(|(other, other_name, other_visibility)| {
            other.name != *owner
                && other_name == name
                && (*other_visibility == Visibility::Private || name.starts_with('#'))
                && (visibility == Visibility::Private || name.starts_with('#'))
        })
    {
        return Some(line(
            depth,
            if name.starts_with('#') {
                format!("Property '{name}' in type '{a}' refers to a different member that cannot be accessed from within type '{e}'.")
            } else {
                format!("Types have separate declarations of a private property '{name}'.")
            },
        ));
    }
    let (class, name, visibility) = restricted
        .iter()
        .find(|(_, _, visibility)| *visibility == Visibility::Private)
        .unwrap_or(&(class, name.clone(), visibility))
        .clone();
    let owner = &class.name;
    Some(line(
        depth,
        if visibility == Visibility::Protected {
            format!(
                "Property '{name}' is protected in type '{owner}' but public in type '{}'.",
                if *owner == a { &e } else { &a }
            )
        } else {
            format!(
                "Property '{name}' is private in type '{owner}' but not in type '{}'.",
                if *owner == a { &e } else { &a }
            )
        },
    ))
}
