// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lossless ECMAScript strings, including ill-formed UTF-16 (§6.1.4).

/// A sequence of UTF-16 code units. Length, equality, hashing and
/// ordering operate on code units, without normalization. Unlike Rust
/// `String`, this can represent lone surrogates. Host UTF-8 conversion is
/// explicit and fallible; it never silently substitutes U+FFFD.
///
/// Cloning shares the buffer (a script reading a variable, a property or a method's
/// receiver clones the value, and with a megabyte of text that copy was the engine's
/// whole run time); changing a string copies it first only if something else still
/// holds it. Each handle also carries the capacity it is *charged* for in the memory
/// accounting: an original is charged for the allocation it was built with, and a clone
/// for its length, exactly as when a clone was a `Vec` copy, so what each holder is
/// charged does not depend on the sharing. The fields are private; nothing else observes
/// either.
pub struct JsString(std::sync::Arc<Vec<u16>>, usize);

impl Clone for JsString {
    fn clone(&self) -> Self {
        Self(std::sync::Arc::clone(&self.0), self.0.len())
    }
}

impl Default for JsString {
    fn default() -> Self {
        Self::from_code_units(Vec::new())
    }
}

impl std::fmt::Debug for JsString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("JsString").field(&*self.0).finish()
    }
}

impl PartialEq for JsString {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for JsString {}

impl PartialOrd for JsString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JsString {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl std::hash::Hash for JsString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl JsString {
    pub fn from_code_units(units: Vec<u16>) -> Self {
        let charged = units.capacity();
        Self(std::sync::Arc::new(units), charged)
    }

    /// The units to change, copied first if another handle shares them. Call
    /// [`Self::settle`] after changing them.
    fn units_mut(&mut self) -> &mut Vec<u16> {
        std::sync::Arc::make_mut(&mut self.0)
    }

    /// A string that has just been changed owns its buffer, so it is charged for it.
    fn settle(&mut self) {
        self.1 = self.0.capacity();
    }

    pub fn as_code_units(&self) -> &[u16] {
        &self.0
    }

    /// Web IDL USVString conversion keeps this same admitted UTF-16 buffer.
    pub(crate) fn replace_unpaired_surrogates(mut self) -> Self {
        let mut index = 0;
        while index < self.0.len() {
            let unit = self.0[index];
            if (0xd800..=0xdbff).contains(&unit)
                && self
                    .0
                    .get(index + 1)
                    .is_some_and(|next| (0xdc00..=0xdfff).contains(next))
            {
                index += 2;
                continue;
            }
            if (0xd800..=0xdfff).contains(&unit) {
                self.units_mut()[index] = 0xfffd;
                self.settle();
            }
            index += 1;
        }
        self
    }

    /// ASCII case folding keeps the already admitted UTF-16 allocation.
    pub(crate) fn make_ascii_lowercase(&mut self) {
        // Only a string with something to lowercase is copied from other holders.
        if self.0.iter().any(|unit| (0x41..=0x5a).contains(unit)) {
            for unit in self.units_mut() {
                if (0x41..=0x5a).contains(unit) {
                    *unit += 0x20;
                }
            }
            self.settle();
        }
    }

    /// The memory this handle is charged for (see the type's documentation).
    pub(crate) fn owned_heap_payload_bytes(&self) -> Option<usize> {
        self.1.checked_mul(std::mem::size_of::<u16>())
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
        let units = self.units_mut();
        if code <= 0xffff {
            units.push(code as u16);
        } else {
            units.push(((code - 0x10000) / 0x400 + 0xd800) as u16);
            units.push(((code - 0x10000) % 0x400 + 0xdc00) as u16);
        }
        self.settle();
    }

    pub(crate) fn push_str(&mut self, other: &Self) {
        if other.0.is_empty() {
            return;
        }
        // `s += s` appends a string to a handle on itself: take the units first.
        let other = std::sync::Arc::clone(&other.0);
        self.units_mut().extend_from_slice(&other);
        self.settle();
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
    pub(crate) fn native_function_source(initial_name: Option<&str>) -> Self {
        const PREFIX: &[u16] = &[
            0x0066, 0x0075, 0x006e, 0x0063, 0x0074, 0x0069, 0x006f, 0x006e, 0x0020,
        ];
        const SUFFIX: &[u16] = &[
            0x0028, 0x0029, 0x0020, 0x007b, 0x0020, 0x005b, 0x006e, 0x0061, 0x0074, 0x0069, 0x0076,
            0x0065, 0x0020, 0x0063, 0x006f, 0x0064, 0x0065, 0x005d, 0x0020, 0x007d,
        ];

        let initial_name = initial_name.filter(|name| Self::is_native_function_name(name));
        let mut units =
            Vec::with_capacity(PREFIX.len() + initial_name.map_or(0, str::len) + SUFFIX.len());
        units.extend_from_slice(PREFIX);
        if let Some(initial_name) = initial_name {
            units.extend(initial_name.encode_utf16());
        }
        units.extend_from_slice(SUFFIX);
        Self::from_code_units(units)
    }

    /// Whether this initial name can appear as `NativeFunctionAccessor_opt
    /// PropertyName_opt` in a NativeFunction: an optional `get `/`set ` prefix
    /// followed by nothing, an IdentifierName, or a bracketed computed name
    /// such as `[Symbol.species]`.
    fn is_native_function_name(name: &str) -> bool {
        let name = name
            .strip_prefix("get ")
            .or_else(|| name.strip_prefix("set "))
            .unwrap_or(name);
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
        for &unit in self.0.iter() {
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
        Self::from_code_units(text.encode_utf16().collect())
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
    fn cloning_shares_the_buffer_and_a_change_copies_it_first() {
        let original = JsString::from("hello");
        let mut copy = original.clone();
        assert!(
            std::ptr::eq(
                original.as_code_units().as_ptr(),
                copy.as_code_units().as_ptr()
            ),
            "a clone is a handle on the same code units"
        );
        copy.push_str(&JsString::from(" world"));
        copy.push_code_point(0x1f600);
        assert_eq!(original, "hello", "the original is untouched");
        assert_eq!(copy.len(), "hello world".len() + 2);
        assert!(!std::ptr::eq(
            original.as_code_units().as_ptr(),
            copy.as_code_units().as_ptr()
        ));
    }

    #[test]
    fn a_string_nobody_else_holds_is_changed_in_place() {
        let mut only = JsString::from_code_units(Vec::with_capacity(64));
        only.push_str(&JsString::from("abc"));
        let before = only.as_code_units().as_ptr();
        only.push_str(&JsString::from("def"));
        assert_eq!(
            only.as_code_units().as_ptr(),
            before,
            "no copy, no reallocation"
        );
        assert_eq!(only, "abcdef");
    }

    #[test]
    fn lowercasing_and_surrogate_repair_leave_other_holders_alone() {
        let upper = JsString::from("ABC");
        let mut lower = upper.clone();
        lower.make_ascii_lowercase();
        assert_eq!(upper, "ABC");
        assert_eq!(lower, "abc");
        let lone = JsString::from_code_units(vec![0x61, 0xd800, 0x62]);
        let repaired = lone.clone().replace_unpaired_surrogates();
        assert_eq!(lone.as_code_units(), &[0x61, 0xd800, 0x62]);
        assert_eq!(repaired.as_code_units(), &[0x61, 0xfffd, 0x62]);
    }

    #[test]
    fn equality_ordering_and_hashing_follow_the_code_units_not_the_handle() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let a = JsString::from("abc");
        let b = JsString::from("abc");
        assert_eq!(a, b);
        let (low, high) = (JsString::from("abc"), JsString::from("abd"));
        assert!(low < high);
        assert_eq!(low.cmp(&a), std::cmp::Ordering::Equal);
        let hash = |s: &JsString| {
            let mut h = DefaultHasher::new();
            s.hash(&mut h);
            h.finish()
        };
        assert_eq!(hash(&a), hash(&b));
    }

    #[test]
    fn an_original_is_charged_for_its_capacity_and_a_copy_for_its_length() {
        let mut units = Vec::with_capacity(19);
        units.extend([97u16, 98]);
        let original = JsString::from_code_units(units);
        assert_eq!(original.owned_heap_payload_bytes(), Some(19 * 2));
        let copy = original.clone();
        assert_eq!(
            copy.owned_heap_payload_bytes(),
            Some(2 * 2),
            "a copy has always been charged as exactly its length"
        );
        assert_eq!(
            original.owned_heap_payload_bytes(),
            Some(19 * 2),
            "and the original is unchanged"
        );
        // A copy that is then changed owns a buffer of its own, charged for it.
        let mut grown = copy.clone();
        grown.push_str(&JsString::from("cd"));
        assert_eq!(
            grown.owned_heap_payload_bytes(),
            Some(grown.0.capacity() * 2)
        );
    }

    #[test]
    fn invalid_native_function_name_is_omitted() {
        assert_eq!(
            JsString::native_function_source(Some("get $&")),
            JsString::native_function_source(None)
        );
    }

    #[test]
    fn an_array_index_rejects_addition_overflow() {
        let overflow = (usize::MAX as u128 + 1).to_string();
        assert_eq!(JsString::from(overflow).index(), None);
    }
}
