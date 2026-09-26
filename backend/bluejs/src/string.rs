// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lossless ECMAScript strings, including ill-formed UTF-16 (§6.1.4).

/// An owned sequence of UTF-16 code units. Length, equality, hashing and
/// ordering operate on code units, without normalization. Unlike Rust
/// `String`, this can represent lone surrogates. Host UTF-8 conversion is
/// explicit and fallible; it never silently substitutes U+FFFD.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JsString(Vec<u16>);

impl JsString {
    pub fn from_code_units(units: Vec<u16>) -> Self {
        Self(units)
    }

    pub fn as_code_units(&self) -> &[u16] {
        &self.0
    }

    /// Number of code units, not bytes or Unicode scalar values.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn to_utf8(&self) -> Result<String, std::string::FromUtf16Error> {
        String::from_utf16(&self.0)
    }

    pub(crate) fn byte_len(&self) -> usize {
        self.0.len() * std::mem::size_of::<u16>()
    }

    // UTF16EncodeCodePoint (§11.1.1), including surrogate code points.
    // The lexer validates the upper bound before calling this operation.
    pub(crate) fn push_code_point(&mut self, code: u32) {
        debug_assert!(code <= 0x10ffff);
        if code <= 0xffff {
            self.0.push(code as u16);
        } else {
            self.0.push(((code - 0x10000) / 0x400 + 0xd800) as u16);
            self.0.push(((code - 0x10000) % 0x400 + 0xdc00) as u16);
        }
    }

    pub(crate) fn push_str(&mut self, other: &Self) {
        self.0.extend_from_slice(&other.0);
    }

    /// The implementation-defined NativeFunction representation used when a
    /// callable's source text is unavailable.  Construct it in one allocated
    /// UTF-16 buffer: `Function.prototype.toString` is frequently used by
    /// framework feature detection, and neither the fixed fragments nor the
    /// immutable initial name need intermediate `JsString` allocations.
    ///
    /// The result must match the NativeFunction grammar, whose only name slot
    /// is `NativeFunctionAccessor_opt PropertyName_opt`. An initial name that
    /// grammar cannot spell (for example the legacy `RegExp.$&` accessors'
    /// `get $&`) is therefore left out rather than producing invalid source.
    pub(crate) fn native_function_source(initial_name: Option<&Self>) -> Self {
        const PREFIX: &[u16] = &[
            0x0066, 0x0075, 0x006e, 0x0063, 0x0074, 0x0069, 0x006f, 0x006e, 0x0020,
        ];
        const SUFFIX: &[u16] = &[
            0x0028, 0x0029, 0x0020, 0x007b, 0x0020, 0x005b, 0x006e, 0x0061, 0x0074, 0x0069, 0x0076,
            0x0065, 0x0020, 0x0063, 0x006f, 0x0064, 0x0065, 0x005d, 0x0020, 0x007d,
        ];

        let initial_name = initial_name.filter(|name| name.is_native_function_name());
        let mut units =
            Vec::with_capacity(PREFIX.len() + initial_name.map_or(0, Self::len) + SUFFIX.len());
        units.extend_from_slice(PREFIX);
        if let Some(initial_name) = initial_name {
            units.extend_from_slice(initial_name.as_code_units());
        }
        units.extend_from_slice(SUFFIX);
        Self(units)
    }

    /// Whether this initial name can appear as `NativeFunctionAccessor_opt
    /// PropertyName_opt` in a NativeFunction: an optional `get `/`set ` prefix
    /// followed by nothing, an IdentifierName, or a bracketed computed name
    /// such as `[Symbol.species]`.
    fn is_native_function_name(&self) -> bool {
        let Ok(name) = self.to_utf8() else {
            return false;
        };
        let name = name
            .strip_prefix("get ")
            .or_else(|| name.strip_prefix("set "))
            .unwrap_or(&name);
        let mut characters = name.chars();
        match characters.next() {
            None => true,
            Some('[') => name.ends_with(']'),
            Some(first) => {
                crate::token::is_ident_start(first)
                    && characters.all(crate::token::is_ident_continue)
            }
        }
    }

    pub(crate) fn index(&self) -> Option<usize> {
        if self.is_empty() || (self.len() > 1 && self.0[0] == u16::from(b'0')) {
            return None;
        }
        let mut index = 0usize;
        for &unit in &self.0 {
            if !(u16::from(b'0')..=u16::from(b'9')).contains(&unit) {
                return None;
            }
            index = index
                .checked_mul(10)?
                .checked_add(usize::from(unit) - usize::from(b'0'))?;
        }
        Some(index)
    }

    pub(crate) fn own_property(&self, key: &Self) -> Option<crate::Value> {
        if key == "length" {
            return Some(crate::Value::Number(self.len() as f64));
        }
        let unit = self.0.get(key.index()?)?;
        Some(crate::Value::String(Self::from_code_units(vec![*unit])))
    }
}

impl From<&str> for JsString {
    fn from(text: &str) -> Self {
        Self(text.encode_utf16().collect())
    }
}

impl From<String> for JsString {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}

impl From<&JsString> for JsString {
    fn from(text: &JsString) -> Self {
        text.clone()
    }
}

impl From<&String> for JsString {
    fn from(text: &String) -> Self {
        Self::from(text.as_str())
    }
}

impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        self.0.iter().copied().eq(other.encode_utf16())
    }
}

impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

#[cfg(test)]
mod tests {
    use super::JsString;

    #[test]
    fn invalid_utf16_cannot_be_a_native_function_name() {
        let invalid_name = JsString::from_code_units(vec![0xD800]);
        assert_eq!(
            JsString::native_function_source(Some(&invalid_name)),
            JsString::native_function_source(None)
        );
    }

    #[test]
    fn an_array_index_rejects_addition_overflow() {
        let overflow = (usize::MAX as u128 + 1).to_string();
        assert_eq!(JsString::from(overflow).index(), None);
    }
}
