// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Explain tuple length, required slots and element incompatibilities.
use super::*;
use crate::{Project, TupleTypeElement};

pub(super) fn reason(
    source: &Type,
    target: &Type,
    project: &Project,
    depth: usize,
) -> Option<String> {
    if let (Type::Tuple(elements), Type::Array(target)) = (source, target) {
        let mut values = Vec::new();
        for element in elements {
            let value = if element.rest {
                if let Type::Array(value) = &element.annotation {
                    *value.clone()
                } else {
                    element.annotation.clone()
                }
            } else {
                element.annotation.clone()
            };
            if !values.contains(&value) {
                values.push(value);
            }
        }
        let value = if values.len() == 1 {
            values.pop().unwrap()
        } else {
            Type::Union(values)
        };
        return Some(relations::mismatch(&value, target, project, depth));
    }
    let Type::Tuple(target) = target else {
        return None;
    };
    if let Type::Array(_) = source {
        if let Some(index) = target
            .iter()
            .position(|element| !element.rest && !element.optional)
        {
            return Some(relations::line(
                depth,
                format!(
                    "Source provides no match for required element at position {index} in target."
                ),
            ));
        }
        return Some(String::new());
    }
    let Type::Tuple(source) = source else {
        return None;
    };
    let min = |elements: &[TupleTypeElement]| {
        elements
            .iter()
            .filter(|element| !element.optional && !element.rest)
            .count()
    };
    let smin = min(source);
    let tmin = min(target);
    let sr = source.iter().position(|element| element.rest);
    let tr = target.iter().position(|element| element.rest);
    let message = if sr.is_none() && source.len() < tmin {
        Some(format!(
            "Source has {} element(s) but target requires {tmin}.",
            source.len()
        ))
    } else if tr.is_none() && smin > target.len() {
        Some(format!(
            "Source has {smin} element(s) but target allows only {}.",
            target.len()
        ))
    } else if sr.is_some() && tr.is_none() && tmin > smin {
        Some(format!(
            "Target requires {tmin} element(s) but source may have fewer."
        ))
    } else if sr.is_some() && tr.is_none() {
        Some(format!(
            "Target allows only {} element(s) but source may have more.",
            target.len()
        ))
    } else {
        None
    };
    if let Some(message) = message {
        return Some(relations::line(depth, message));
    }
    // Required suffix elements cannot be guaranteed by an optional or unbounded source.
    if let Some(rest) = tr.filter(|_| sr.is_none()) {
        for index in rest + 1..target.len() {
            if target[index].optional {
                continue;
            }
            let offset = target.len() - index;
            let matched = source
                .len()
                .checked_sub(offset)
                .and_then(|index| source.get(index));
            if matched.is_none_or(|element| element.optional || element.rest) {
                return Some(relations::line(depth,format!("Source provides no match for required element at position {index} in target.")));
            }
        }
    }
    if let (Some(sr), Some(tr)) = (sr, tr) {
        if sr < tr && target[sr..tr].iter().any(|element| !element.optional) {
            return Some(relations::line(
                depth,
                format!(
                    "Source provides no match for required element at position {sr} in target."
                ),
            ));
        }
    }
    for (index, element) in source.iter().enumerate() {
        let target_index = if let Some(rest) = tr {
            if index >= source.len().saturating_sub(target.len() - rest - 1)
                && index > rest
                && target.len() - rest > 1
            {
                target.len() - (source.len() - index)
            } else if index >= rest && (sr.is_none() || index == sr.unwrap()) {
                rest
            } else {
                index.min(target.len().saturating_sub(1))
            }
        } else {
            index
        };
        let Some(expected) = target.get(target_index) else {
            continue;
        };
        let element_value = if element.rest {
            if let Type::Array(value) = &element.annotation {
                value.as_ref()
            } else {
                &element.annotation
            }
        } else {
            &element.annotation
        };
        let expected_value = if expected.rest {
            if let Type::Array(value) = &expected.annotation {
                value.as_ref()
            } else {
                &expected.annotation
            }
        } else {
            &expected.annotation
        };
        if !relations::contains(expected_value, element_value) {
            let positions = if expected.rest
                && sr.is_none()
                && tr.is_some_and(|rest| {
                    source.len().saturating_sub(target.len() - rest - 1) > rest + 1
                }) {
                let rest = tr.expect("rest target position");
                let end = source.len().saturating_sub(target.len() - rest - 1) - 1;
                format!("Type at positions {rest} through {end} in source is not compatible with type at position {target_index} in target.")
            } else if element.optional
                && expected.rest
                && index + 1 < source.len()
                && source[index + 1].optional
            {
                format!("Type at positions {index} through {} in source is not compatible with type at position {target_index} in target.",source.iter().take_while(|element|element.optional).count()-1)
            } else {
                format!("Type at position {index} in source is not compatible with type at position {target_index} in target.")
            };
            let expected = if expected.optional {
                if let Type::Union(values) = expected_value {
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
                    expected_value.clone()
                }
            } else {
                expected_value.clone()
            };
            return Some(
                relations::line(depth, positions)
                    + &relations::mismatch(element_value, &expected, project, depth + 1),
            );
        }
    }
    Some(String::new())
}
