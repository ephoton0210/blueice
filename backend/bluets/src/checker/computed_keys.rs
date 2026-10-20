// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Symbol properties share source identities, never their display strings.
use super::*;

pub(super) fn same(left: &str, right: &str, aliases: &BTreeMap<String, TypeDefinition>) -> bool {
    if left == right {
        return true;
    }
    matches!((key(left, aliases), key(right, aliases)),
        (Some(Type::UniqueSymbol(a)), Some(Type::UniqueSymbol(b))) if a == b)
}

pub(super) fn key<'a>(
    name: &str,
    aliases: &'a BTreeMap<String, TypeDefinition>,
) -> Option<&'a Type> {
    crate::symbol_keys::entity(name)?;
    aliases.get(name).map(|definition| &definition.value)
}

pub(super) fn applies(name: &str, index: &Type) -> bool {
    match index {
        Type::Symbol => crate::symbol_keys::entity(name).is_some(),
        Type::Number => name.parse::<f64>().is_ok(),
        Type::String => crate::symbol_keys::entity(name).is_none(),
        _ => true,
    }
}

pub(super) fn broad_indices(
    value: &Type,
    aliases: &BTreeMap<String, TypeDefinition>,
) -> Option<Type> {
    let Type::Record(fields) = value else {
        return None;
    };
    if !fields
        .iter()
        .any(|field| key(&field.name, aliases) == Some(&Type::Symbol))
    {
        return None;
    }
    let mut object = Vec::new();
    let mut indices = Vec::new();
    for field in fields {
        if key(&field.name, aliases) == Some(&Type::Symbol) {
            indices.push(crate::parser::IndexSignature {
                name: "x".into(),
                key: Type::Symbol,
                value: field.value.clone(),
                readonly: field.readonly,
                key_span: field.span.clone(),
                span: field.span.clone(),
            });
        } else {
            object.push(field.clone());
        }
    }
    Some(Type::IndexedRecord {
        object: Box::new(Type::Record(object)),
        indices,
    })
}
