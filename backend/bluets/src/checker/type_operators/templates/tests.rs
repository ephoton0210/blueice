// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;
use crate::parser::{StringMappingType, TypeParameter};
use crate::{CaseMapping, SourceSpan};

#[path = "../../../../tests/fixtures/string_intrinsics/template-native-vectors.rs"]
mod native;
#[path = "../../../../tests/fixtures/string_intrinsics/template-numeric-vectors.rs"]
mod numeric_native;

fn pattern(value: &str) -> Type {
    match value {
        "string" => Type::String,
        "number" => Type::Number,
        "Uppercase<string>" => Type::StringMapping(Box::new(StringMappingType {
            operation: CaseMapping::Upper,
            origin: SourceSpan::new("native-vector", 0, 1),
            operand: Type::String,
        })),
        _ => panic!("unexpected Native vector pattern: {value}"),
    }
}

#[test]
fn native_template_membership_retains_units_and_first_delimiter_selection() {
    assert_eq!(native::MATCHES.len(), 10);
    for &(id, literal, head, patterns, tails, wanted) in native::MATCHES {
        let template = TemplateLiteralType {
            head: head.into(),
            spans: patterns
                .iter()
                .zip(tails)
                .map(|(p, tail)| (pattern(p), (*tail).into()))
                .collect(),
            span: SourceSpan::new(id, 0, 1),
            generated: false,
        };
        assert_eq!(
            matches(
                &Type::Literal(literal.into()),
                &template,
                &mut conditional::MatchBindings::default(),
                &BTreeMap::new(),
                &mut TypeExpansionBudget::new(256)
            ),
            wanted,
            "{id}"
        );
    }
}

#[test]
fn native_inference_splits_the_first_utf16_unit_without_lossy_conversion() {
    let infer = |name: &str| {
        Type::Infer(Box::new(TypeParameter {
            is_const: false,
            variance: None,
            name: name.into(),
            constraint: None,
            default: None,
            span: SourceSpan::new("native-vector", 0, 1),
        }))
    };
    let template = TemplateLiteralType {
        head: String::new(),
        spans: vec![(infer("H"), String::new()), (infer("T"), String::new())],
        span: SourceSpan::new("native-vector", 0, 1),
        generated: false,
    };
    let mut bindings = conditional::MatchBindings::default();
    assert!(matches(
        &Type::Literal(r#""\uD801\uDC28x""#.into()),
        &template,
        &mut bindings,
        &BTreeMap::new(),
        &mut TypeExpansionBudget::new(256)
    ));
    assert_eq!(
        bindings["H"].string_value().unwrap().units(),
        native::FIRST_UNIT_HEAD
    );
    assert_eq!(
        bindings["T"].string_value().unwrap().units(),
        native::FIRST_UNIT_TAIL
    );
    assert!(!matches(
        &Type::Literal(r#""""#.into()),
        &template,
        &mut conditional::MatchBindings::default(),
        &BTreeMap::new(),
        &mut TypeExpansionBudget::new(256)
    ));
}

#[test]
fn numeric_template_placeholders_match_all_additional_native_vectors() {
    assert_eq!(numeric_native::VECTORS.len(), 52);
    for &(kind, text, wanted) in numeric_native::VECTORS {
        let units = Utf16String::from(text);
        let actual = match kind {
            "number" => numeric::number(units.units()).is_some(),
            "bigint" => numeric::bigint(units.units()),
            _ => panic!("unknown numeric placeholder"),
        };
        assert_eq!(actual, wanted, "{kind}: {text:?}");
    }
}
