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

    pub(crate) fn index(&self) -> Option<usize> {
        if self.is_empty() || (self.len() > 1 && self.0[0] == u16::from(b'0')) {
            return None;
        }
        let mut index = 0usize;
        for &unit in &self.0 {
            if !(u16::from(b'0')..=u16::from(b'9')).contains(&unit) {
                return None;
            }
            index = index.checked_mul(10)?.checked_add(usize::from(unit) - usize::from(b'0'))?;
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
