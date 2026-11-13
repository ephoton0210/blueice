// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lossless type-level string values; UTF-8 conversion belongs to serialization.

#[path = "utf16/case.rs"]
mod case;
#[path = "utf16/literal.rs"]
mod literal;
#[path = "utf16/render.rs"]
mod render;
#[path = "utf16/value.rs"]
mod value;
pub use value::StringLiteral;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaseMapping {
    Upper,
    Lower,
    Capitalize,
    Uncapitalize,
}

impl CaseMapping {
    pub fn name(self) -> &'static str {
        match self {
            Self::Upper => "Uppercase",
            Self::Lower => "Lowercase",
            Self::Capitalize => "Capitalize",
            Self::Uncapitalize => "Uncapitalize",
        }
    }
}

/// Equality, ordering and hashing inspect code units without normalization.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Utf16String(Vec<u16>);

impl Utf16String {
    pub fn from_units(units: Vec<u16>) -> Self {
        Self(units)
    }

    pub fn units(&self) -> &[u16] {
        &self.0
    }

    pub fn parse_literal(source: &str) -> Option<Self> {
        if !source.starts_with(['\'', '"']) {
            return None;
        }
        literal::parse(source).map(Self)
    }

    pub fn parse_template_chunk(source: &str) -> Option<Self> {
        literal::parse(&format!("`{source}`")).map(Self)
    }

    /// Internal raw syntax retains every unit; inferred printers cook it at
    /// the final boundary rather than replacing malformed units in the type.
    pub fn template_source(&self) -> String {
        self.0.iter().map(|unit| format!("\\u{unit:04X}")).collect()
    }

    pub fn native_template_chunk(&self) -> String {
        render::template_chunk(&self.0)
    }

    pub fn mapped(&self, operation: CaseMapping) -> Self {
        Self(case::map(&self.0, operation))
    }

    /// Native's generated double-quoted token after its UTF-8 writer runs.
    /// This projection must never replace the semantic code units themselves.
    pub fn native_literal(&self) -> String {
        render::literal(&self.0)
    }
}

impl From<&str> for Utf16String {
    fn from(value: &str) -> Self {
        Self(value.encode_utf16().collect())
    }
}

#[cfg(test)]
#[path = "../tests/fixtures/string_intrinsics/utf16-native-vectors.rs"]
pub(crate) mod native_vectors;

#[cfg(test)]
#[path = "../tests/fixtures/string_intrinsics/utf16-native-escapes.rs"]
mod native_escapes;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_semantic_units_and_physical_literals_match_all_frozen_vectors() {
        assert_eq!(native_vectors::VECTORS.len(), 94);
        for &(id, operation, source, input, mapped, token) in native_vectors::VECTORS {
            let value = Utf16String::parse_literal(source).unwrap();
            assert_eq!(value.units(), input, "{id}: source code units");
            let actual = value.mapped(operation);
            assert_eq!(actual.units(), mapped, "{id}: semantic mapping");
            assert_eq!(actual.native_literal(), token, "{id}: physical token");
            assert_eq!(value.units(), input, "{id}: input was mutated");
        }
    }

    #[test]
    fn native_unicode_escape_values_keep_leading_zeroes_and_surrogates() {
        for &(source, units) in native_escapes::ESCAPES {
            assert_eq!(Utf16String::parse_literal(source).unwrap().units(), units);
        }
    }

    #[test]
    fn invalid_escapes_and_unterminated_literals_are_not_lossy_values() {
        for source in [
            "\"unterminated",
            r#""\u{110000}""#,
            r#""\x0g""#,
            r#""\uZZZZ""#,
        ] {
            assert!(Utf16String::parse_literal(source).is_none(), "{source}");
        }
    }
}
