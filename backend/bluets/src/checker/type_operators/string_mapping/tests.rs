// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

use crate::utf16::native_vectors;

fn operation(value: Type, operation: CaseMapping, module: &str) -> Type {
    Type::StringMapping(Box::new(StringMappingType {
        operation,
        origin: crate::SourceSpan::new(module, 1, 10),
        operand: value,
    }))
}

fn mapped(value: &Type) -> Type {
    expanded(
        value,
        &BTreeMap::new(),
        &mut HashSet::new(),
        &mut TypeExpansionBudget::new(256),
    )
}

#[test]
fn actual_operator_retains_all_native_semantic_units_and_physical_tokens() {
    for &(id, kind, source, _, units, token) in native_vectors::VECTORS {
        let actual = mapped(&operation(Type::Literal(source.into()), kind, "owner"));
        let Type::StringLiteral(actual) = actual else {
            panic!("{id}: {actual:?}");
        };
        assert_eq!(actual.value().units(), units, "{id}");
        assert_eq!(actual.declaration_text(), token, "{id}");
    }
}

#[test]
fn union_mapping_reduces_never_and_deduplicates_generated_values() {
    let actual = mapped(&operation(
        Type::Union(vec![
            Type::Literal("\"a\"".into()),
            Type::Never,
            Type::Literal("'A'".into()),
        ]),
        CaseMapping::Upper,
        "owner",
    ));
    assert_eq!(actual.string_value().unwrap().units(), &[65]);
}

#[test]
fn repeated_mapping_retains_defining_symbols_and_operation_order() {
    let inner = operation(Type::String, CaseMapping::Upper, "one");
    assert_eq!(
        mapped(&operation(inner.clone(), CaseMapping::Upper, "one")),
        inner
    );
    for (kind, owner) in [(CaseMapping::Upper, "two"), (CaseMapping::Lower, "one")] {
        let different = operation(inner.clone(), kind, owner);
        assert_eq!(mapped(&different), different);
    }
}

#[test]
fn lexical_parameter_aliases_do_not_turn_into_their_broad_constraint() {
    let reference = |name: &str| Type::Named {
        name: name.into(),
        arguments: Vec::new(),
    };
    let aliases = BTreeMap::from([
        (
            "T".into(),
            TypeDefinition {
                kind: TypeDefinitionKind::Alias,
                parameters: Vec::new(),
                value: reference("lexical-T"),
            },
        ),
        (
            "lexical-T".into(),
            TypeDefinition {
                kind: TypeDefinitionKind::Parameter,
                parameters: Vec::new(),
                value: Type::String,
            },
        ),
    ]);
    let value = expanded(
        &operation(reference("T"), CaseMapping::Upper, "owner"),
        &aliases,
        &mut HashSet::new(),
        &mut TypeExpansionBudget::new(100),
    );
    assert_eq!(
        value,
        operation(reference("lexical-T"), CaseMapping::Upper, "owner")
    );
}
