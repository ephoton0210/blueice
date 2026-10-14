// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parameter reasons for already-rejected function assignments.
use super::*;
use crate::{Project, TupleTypeElement};

pub(super) fn reason(
    actual: &[Parameter],
    expected: &[Parameter],
    project: &Project,
    depth: usize,
) -> String {
    let a = parameters::expanded(actual, Some(project), true);
    let e = parameters::expanded(expected, Some(project), true);
    let tuple_rest = |parameters: &[Parameter]| {
        parameters
            .iter()
            .any(|parameter| parameter.rest && matches!(parameter.annotation, Some(Type::Tuple(_))))
    };
    let required = |parameters: &[Parameter]| {
        parameters
            .iter()
            .map(|parameter| {
                if parameter.rest {
                    if let Some(Type::Tuple(elements)) = &parameter.annotation {
                        elements
                            .iter()
                            .filter(|element| !element.optional && !element.rest)
                            .count()
                    } else {
                        0
                    }
                } else {
                    usize::from(!parameter.optional && parameter.default.is_none())
                }
            })
            .sum::<usize>()
    };
    let count = |parameters: &[Parameter], original: &[Parameter]| {
        if tuple_rest(parameters) {
            let Type::Tuple(elements) = tuple(parameters, original) else {
                unreachable!()
            };
            elements
                .iter()
                .filter(|element| !element.optional && !element.rest)
                .count()
        } else {
            required(parameters)
        }
    };
    let ar = count(&a, actual);
    let er = count(&e, expected);
    if ar > e.len() && !e.iter().any(|parameter| parameter.rest) && !tuple_rest(&a) {
        return relations::line(
            depth,
            format!(
                "Target signature provides too few arguments. Expected {ar} or more, but got {}.",
                e.len()
            ),
        );
    }
    let tuple_pair = (tuple_rest(&a) || tuple_rest(&e))
        && (ar > e.len() && !e.iter().any(|parameter| parameter.rest)
            || er > a.len() && !a.iter().any(|parameter| parameter.rest));
    if tuple_pair && !a.is_empty() && !e.is_empty() {
        return parameter_reason(
            &a[0].name,
            &e[0].name,
            &tuple(&e, expected),
            &tuple(&a, actual),
            project,
            depth,
        );
    }
    let mut ai = 0;
    let mut ei = 0;
    for _ in 0..a.len() + e.len() + 1 {
        let (Some(ap), Some(ep)) = (a.get(ai), e.get(ei)) else {
            break;
        };
        let at = ap.annotation.as_ref().unwrap_or(&Type::Any);
        let et = ep.annotation.as_ref().unwrap_or(&Type::Any);
        if matches!(at, Type::Tuple(_)) && ap.rest || matches!(et, Type::Tuple(_)) && ep.rest {
            let av = if ap.rest {
                at.clone()
            } else {
                tuple(&a[ai..], actual)
            };
            let ev = if ep.rest {
                et.clone()
            } else {
                tuple(&e[ei..], expected)
            };
            let detail = relations::reason(&ev, &av, project, depth + 1);
            if !detail.is_empty() {
                return parameter_reason(&ap.name, &ep.name, &ev, &av, project, depth);
            }
            break;
        }
        let mut av = if let Type::Array(value) = at {
            if ap.rest {
                *value.clone()
            } else {
                at.clone()
            }
        } else {
            at.clone()
        };
        let mut ev = if let Type::Array(value) = et {
            if ep.rest {
                *value.clone()
            } else {
                et.clone()
            }
        } else {
            et.clone()
        };
        if ap.optional || ap.default.is_some() {
            av = parameters::optional(&av);
        }
        if ep.optional || ep.default.is_some() {
            ev = parameters::optional(&ev);
        }
        if !relations::contains(&av, &ev) {
            if (ap.optional || ap.default.is_some())
                && !ep.optional
                && ep.default.is_none()
                && !(ep.rest && ei + 1 == e.len() && ai > ei)
            {
                av = without_undefined(av);
            }
            return parameter_reason(&ap.name, &ep.name, &ev, &av, project, depth);
        }
        if !ap.rest {
            ai += 1;
        }
        if !ep.rest {
            ei += 1;
        }
        if ap.rest && ep.rest {
            break;
        }
    }
    String::new()
}
fn without_undefined(value: Type) -> Type {
    if let Type::Union(values) = value {
        let mut values = values
            .into_iter()
            .filter(|value| *value != Type::Undefined)
            .collect::<Vec<_>>();
        if values.len() == 1 {
            values.pop().unwrap()
        } else {
            Type::Union(values)
        }
    } else {
        value
    }
}
fn parameter_reason(
    a: &str,
    e: &str,
    source: &Type,
    target: &Type,
    project: &Project,
    depth: usize,
) -> String {
    relations::line(
        depth,
        format!("Types of parameters '{a}' and '{e}' are incompatible."),
    ) + &relations::mismatch(source, target, project, depth + 1)
}
fn tuple(parameters: &[Parameter], original: &[Parameter]) -> Type {
    let mut elements = Vec::new();
    for parameter in parameters {
        let value = parameter.annotation.as_ref().unwrap_or(&Type::Any);
        if parameter.rest {
            if let Type::Tuple(values) = value {
                elements.extend(values.clone());
                continue;
            }
        }
        let original = original
            .iter()
            .find(|original| original.span == parameter.span);
        let label = if original.is_some_and(|original| {
            original.rest && !matches!(original.annotation, Some(Type::Array(_)))
        }) {
            match original.unwrap().annotation.as_ref() {
                Some(Type::Tuple(values)) => values
                    .iter()
                    .find(|value| value.label.as_deref() == Some(&parameter.name))
                    .and_then(|value| value.label.clone()),
                _ => None,
            }
        } else {
            Some(parameter.name.clone())
        };
        elements.push(TupleTypeElement {
            annotation: if parameter.optional {
                parameters::optional(value)
            } else {
                value.clone()
            },
            optional: parameter.optional,
            label,
            rest: parameter.rest,
        });
    }
    if elements
        .iter()
        .any(|element| !element.optional && !element.rest)
    {
        if let Some(last_required) = elements
            .iter()
            .rposition(|element| !element.optional && !element.rest)
        {
            for element in &mut elements[..last_required] {
                element.optional = false;
            }
        }
    }
    Type::Tuple(elements)
}
