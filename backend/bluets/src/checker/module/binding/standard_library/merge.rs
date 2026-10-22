// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A refined library layer replaces obsolete members, never owner declarations.
use super::*;
use crate::parser::{IndexSignature, TypeSignature};

pub(super) fn parameters(existing: &mut Vec<TypeParameter>, declarations: &[TypeParameter]) {
    if declarations.is_empty() {
        return;
    }
    let mut parameters = declarations.to_vec();
    for parameter in &mut parameters {
        if let Some(old) = existing.iter().find(|old| old.name == parameter.name) {
            parameter.variance = parameter.variance.or(old.variance);
            if parameter.constraint.is_none() {
                parameter.constraint = old.constraint.clone();
            }
            if parameter.default.is_none() {
                parameter.default = old.default.clone();
            }
        }
    }
    *existing = parameters;
}

#[derive(Default)]
struct Surface {
    bases: Vec<Type>,
    fields: Vec<TypeField>,
    signatures: Vec<TypeSignature>,
    indices: Vec<IndexSignature>,
}

fn surface(value: Type) -> Option<Surface> {
    match value {
        Type::Record(fields) => Some(Surface {
            fields,
            ..Surface::default()
        }),
        Type::CallableRecord { fields, signatures } => Some(Surface {
            fields,
            signatures,
            ..Surface::default()
        }),
        Type::IndexedRecord { object, indices } => {
            let mut result = surface(*object)?;
            result.indices.extend(indices);
            Some(result)
        }
        Type::Intersection(parts) => {
            let mut result = Surface::default();
            for part in parts {
                if let Some(next) = surface(part.clone()) {
                    result.bases.extend(next.bases);
                    result.fields.extend(next.fields);
                    result.signatures.extend(next.signatures);
                    result.indices.extend(next.indices);
                } else {
                    result.bases.push(part);
                }
            }
            Some(result)
        }
        _ => None,
    }
}

pub(super) fn definition(existing: &mut Type, value: Type, replace: bool) {
    let (Some(mut old), Some(new)) = (surface(existing.clone()), surface(value.clone())) else {
        *existing = value;
        return;
    };
    if replace {
        let names: BTreeSet<_> = new.fields.iter().map(|field| &field.name).collect();
        old.fields.retain(|field| !names.contains(&field.name));
        old.indices
            .retain(|index| !new.indices.iter().any(|new| new.key == index.key));
    }
    old.fields.extend(new.fields);
    for base in new.bases {
        if !old.bases.contains(&base) {
            old.bases.push(base);
        }
    }
    let mut signatures = new.signatures;
    signatures.extend(old.signatures);
    old.signatures = signatures;
    old.indices.extend(new.indices);
    let object = if old.signatures.is_empty() {
        Type::Record(old.fields)
    } else {
        Type::CallableRecord {
            fields: old.fields,
            signatures: old.signatures,
        }
    };
    let object = if old.indices.is_empty() {
        object
    } else {
        Type::IndexedRecord {
            object: Box::new(object),
            indices: old.indices,
        }
    };
    *existing = if old.bases.is_empty() {
        object
    } else {
        let mut parts = vec![object];
        parts.extend(old.bases);
        Type::Intersection(parts)
    };
}
