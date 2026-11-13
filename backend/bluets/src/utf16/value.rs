// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Literal identity is independent of source quotes and generated spelling.

use super::{CaseMapping, Utf16String};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct StringLiteral {
    value: Utf16String,
    source: Option<String>,
}

impl StringLiteral {
    pub fn from_source(source: &str) -> Option<Self> {
        Some(Self {
            value: Utf16String::parse_literal(source)?,
            source: Some(source.to_string()),
        })
    }

    pub fn generated(value: Utf16String) -> Self {
        Self {
            value,
            source: None,
        }
    }

    pub fn value(&self) -> &Utf16String {
        &self.value
    }

    pub fn declaration_text(&self) -> String {
        self.source
            .clone()
            .unwrap_or_else(|| self.value.native_literal())
    }

    pub fn diagnostic_text(&self) -> String {
        self.value.native_literal()
    }

    pub fn mapped(&self, operation: CaseMapping) -> Self {
        Self::generated(self.value.mapped(operation))
    }
}

impl PartialEq for StringLiteral {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for StringLiteral {}

impl Hash for StringLiteral {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn source_spelling_is_preserved_without_changing_semantic_identity() {
        let source = StringLiteral::from_source(r#"'\u0061'"#).unwrap();
        let double = StringLiteral::from_source(r#""a""#).unwrap();
        let generated = StringLiteral::generated(Utf16String::from("a"));
        assert_eq!(source, double);
        assert_eq!(source, generated);
        assert_eq!(HashSet::from([source.clone(), double, generated]).len(), 1);
        assert_eq!(source.declaration_text(), r#"'\u0061'"#);
        assert_eq!(source.diagnostic_text(), r#""a""#);
        assert_eq!(
            source.mapped(CaseMapping::Upper).declaration_text(),
            r#""A""#
        );
        assert_eq!(source.declaration_text(), r#"'\u0061'"#);
    }

    #[test]
    fn equal_physical_tokens_do_not_equate_distinct_surrogate_values() {
        let high = StringLiteral::from_source(r#""\uD800""#).unwrap();
        let low = StringLiteral::from_source(r#""\uDC00""#).unwrap();
        let replacement = StringLiteral::from_source(r#""\uFFFD""#).unwrap();
        for operation in [
            CaseMapping::Upper,
            CaseMapping::Lower,
            CaseMapping::Capitalize,
            CaseMapping::Uncapitalize,
        ] {
            let high = high.mapped(operation);
            let low = low.mapped(operation);
            let replacement = replacement.mapped(operation);
            assert_eq!(high.declaration_text(), replacement.declaration_text());
            assert_eq!(low.declaration_text(), replacement.declaration_text());
            assert_eq!(HashSet::from([high, low, replacement]).len(), 3);
        }
    }

    #[test]
    fn source_and_generated_paired_units_have_the_same_value() {
        let source = StringLiteral::from_source(r#"'\uD801\uDC28'"#).unwrap();
        let scalar = StringLiteral::from_source("\"𐐨\"").unwrap();
        assert_eq!(source, scalar);
        assert_eq!(source.value().units(), &[0xd801, 0xdc28]);
        assert_eq!(source.mapped(CaseMapping::Upper).diagnostic_text(), "\"𐐀\"");
        assert_eq!(source.mapped(CaseMapping::Capitalize), scalar);
    }
}
