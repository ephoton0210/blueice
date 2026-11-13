// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Intrinsic definitions retain their declaring symbol across imported aliases.

use super::{SourceSpan, Type};
use crate::utf16::{CaseMapping, Utf16String};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicKind {
    StringMapping(CaseMapping),
    NoInfer,
    BuiltinIteratorReturn,
}

impl IntrinsicKind {
    pub(crate) fn alias(name: &str, parameters: usize) -> Option<Self> {
        match (name, parameters) {
            ("Uppercase", 1) => Some(Self::StringMapping(CaseMapping::Upper)),
            ("Lowercase", 1) => Some(Self::StringMapping(CaseMapping::Lower)),
            ("Capitalize", 1) => Some(Self::StringMapping(CaseMapping::Capitalize)),
            ("Uncapitalize", 1) => Some(Self::StringMapping(CaseMapping::Uncapitalize)),
            ("NoInfer", 1) => Some(Self::NoInfer),
            ("BuiltinIteratorReturn", 0) => Some(Self::BuiltinIteratorReturn),
            _ => None,
        }
    }
}

/// The original keyword stays in syntax trees and emitted declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrinsicType {
    pub kind: IntrinsicKind,
    pub origin: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringMappingType {
    pub operation: CaseMapping,
    pub origin: SourceSpan,
    pub operand: Type,
}

impl Type {
    pub(crate) fn is_literal(&self) -> bool {
        matches!(self, Self::Literal(_) | Self::StringLiteral(_))
    }

    pub(crate) fn is_string_like(&self) -> bool {
        match self {
            Self::String
            | Self::StringLiteral(_)
            | Self::StringMapping(_)
            | Self::TemplateLiteral(_) => true,
            Self::Literal(source) => Utf16String::parse_literal(source).is_some(),
            _ => false,
        }
    }

    /// Raw literal fragments also serve the declaration renderer. Decode only
    /// a complete quoted literal, never a rendered type expression.
    pub(crate) fn string_value(&self) -> Option<Utf16String> {
        match self {
            Self::StringLiteral(value) => Some(value.value().clone()),
            Self::Literal(source) => Utf16String::parse_literal(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_intrinsic_names_require_their_exact_definition_arity() {
        for name in [
            "Uppercase",
            "Lowercase",
            "Capitalize",
            "Uncapitalize",
            "NoInfer",
        ] {
            assert!(IntrinsicKind::alias(name, 1).is_some(), "{name}");
            assert_eq!(IntrinsicKind::alias(name, 0), None);
            assert_eq!(IntrinsicKind::alias(name, 2), None);
        }
        assert_eq!(
            IntrinsicKind::alias("BuiltinIteratorReturn", 0),
            Some(IntrinsicKind::BuiltinIteratorReturn)
        );
        assert_eq!(IntrinsicKind::alias("BuiltinIteratorReturn", 1), None);
        assert_eq!(IntrinsicKind::alias("Fake", 1), None);
        assert_eq!(IntrinsicKind::alias("uppercase", 1), None);
        assert_eq!(IntrinsicKind::alias("Shout", 1), None);
    }

    #[test]
    fn only_complete_literals_provide_semantic_string_values() {
        assert_eq!(
            Type::Literal(r#"'\uD800'"#.into())
                .string_value()
                .unwrap()
                .units(),
            &[0xd800]
        );
        for fragment in ["string", "NoInfer<T>", "{ x: string; }", "'a' | 'b'", "123"] {
            assert!(
                Type::Literal(fragment.into()).string_value().is_none(),
                "{fragment}"
            );
        }
    }

    #[test]
    fn generated_and_source_string_literals_keep_their_primitive_classification() {
        use crate::utf16::StringLiteral;
        for units in [vec![], vec![0xd800], vec![0xdc00], vec![0xfffd], vec![65]] {
            let value =
                Type::StringLiteral(StringLiteral::generated(Utf16String::from_units(units)));
            assert!(value.is_literal());
            assert!(value.is_string_like());
        }
        assert!(Type::Literal("123".into()).is_literal());
        assert!(!Type::Literal("123".into()).is_string_like());
        assert!(!Type::String.is_literal());
        assert!(Type::String.is_string_like());
    }
}
