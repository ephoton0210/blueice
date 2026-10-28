// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Only inferred anonymous records receive native absent-property completion.
use super::*;

pub(super) fn complete(parts: &mut [Type], exact: bool, budget: &mut usize) -> bool {
    if parts.len() < 2
        || !parts.iter().all(|part| {
            let Type::Record(fields) = part else {
                return false;
            };
            fields.iter().all(|field| field.span.module == "<inferred>")
        })
    {
        return true;
    }
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    let mut names = Vec::new();
    for part in parts.iter() {
        if let Type::Record(fields) = part {
            for field in fields {
                if !names
                    .iter()
                    .any(|known: &crate::parser::TypeField| known.name == field.name)
                {
                    names.push(field.clone());
                }
            }
        }
    }
    for part in parts.iter_mut() {
        if let Type::Record(fields) = part {
            for field in &names {
                if !fields.iter().any(|present| present.name == field.name) {
                    let mut absent = field.clone();
                    absent.optional = true;
                    absent.readonly = false;
                    absent.value = if exact { Type::Never } else { Type::Undefined };
                    fields.push(absent);
                } else if exact {
                    for field in fields
                        .iter_mut()
                        .filter(|field| field.optional && field.value == Type::Undefined)
                    {
                        field.value = Type::Never;
                    }
                }
            }
        }
    }
    for name in names {
        let mut children = parts
            .iter()
            .map(|part| {
                let Type::Record(fields) = part else {
                    unreachable!()
                };
                fields
                    .iter()
                    .find(|field| field.name == name.name)
                    .expect("completed field")
                    .value
                    .clone()
            })
            .collect::<Vec<_>>();
        if !complete(&mut children, exact, budget) {
            return false;
        }
        for (part, value) in parts.iter_mut().zip(children) {
            let Type::Record(fields) = part else {
                unreachable!()
            };
            fields
                .iter_mut()
                .find(|field| field.name == name.name)
                .expect("completed field")
                .value = value;
        }
    }
    true
}
