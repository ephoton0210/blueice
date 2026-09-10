// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::{JsString, Value};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// Symbol identity is independent of its optional description and of VM heaps.
#[derive(Debug, Clone)]
pub struct JsSymbol {
    id: u64,
    pub(crate) description: Option<JsString>,
}

impl JsSymbol {
    pub fn new(description: Option<JsString>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(64);
        Self {
            id: NEXT
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("Symbol identity space exhausted"),
            description,
        }
    }

    pub fn well_known(name: &str) -> Self {
        let index = WELL_KNOWN
            .iter()
            .position(|&n| n == name)
            .expect("valid well-known Symbol");
        Self {
            id: index as u64 + 1,
            description: Some(format!("Symbol.{name}").into()),
        }
    }

    pub(crate) fn descriptive_string(&self) -> JsString {
        let mut result = JsString::from("Symbol(");
        if let Some(description) = &self.description {
            result.push_str(description);
        }
        result.push_str(&")".into());
        result
    }
}
impl PartialEq for JsSymbol {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for JsSymbol {}
impl Hash for JsSymbol {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

pub(crate) const WELL_KNOWN: &[&str] = &[
    "iterator",
    "match",
    "matchAll",
    "replace",
    "search",
    "split",
    "toPrimitive",
    "toStringTag",
    "species",
    "hasInstance",
    "isConcatSpreadable",
    "unscopables",
    "asyncIterator",
    "dispose",
    "asyncDispose",
];

/// An ECMAScript property key; Symbols never alias string names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PropertyName {
    String(JsString),
    Symbol(JsSymbol),
}

impl PropertyName {
    pub(crate) fn byte_len(&self) -> usize {
        match self {
            Self::String(s) => s.byte_len(),
            Self::Symbol(s) => s.description.as_ref().map_or(0, JsString::byte_len),
        }
    }
    pub(crate) fn index(&self) -> Option<usize> {
        match self {
            Self::String(s) => s.index(),
            _ => None,
        }
    }
    pub(crate) fn value(&self) -> Value {
        match self {
            Self::String(s) => Value::String(s.clone()),
            Self::Symbol(s) => Value::Symbol(s.clone()),
        }
    }
}
impl From<JsString> for PropertyName {
    fn from(s: JsString) -> Self {
        Self::String(s)
    }
}
impl From<&JsString> for PropertyName {
    fn from(s: &JsString) -> Self {
        Self::String(s.clone())
    }
}
impl From<&str> for PropertyName {
    fn from(s: &str) -> Self {
        Self::String(s.into())
    }
}
impl From<String> for PropertyName {
    fn from(s: String) -> Self {
        Self::String(s.into())
    }
}
impl From<&String> for PropertyName {
    fn from(s: &String) -> Self {
        Self::String(s.as_str().into())
    }
}
impl From<&PropertyName> for PropertyName {
    fn from(s: &PropertyName) -> Self {
        s.clone()
    }
}
impl From<JsSymbol> for PropertyName {
    fn from(s: JsSymbol) -> Self {
        Self::Symbol(s)
    }
}
impl PartialEq<str> for PropertyName {
    fn eq(&self, other: &str) -> bool {
        matches!(self, Self::String(s) if s == other)
    }
}
impl PartialEq<&str> for PropertyName {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

/// Partial descriptor accepted by DefineOwnProperty. Absent fields preserve
/// existing attributes; new properties default to false/undefined.
#[derive(Debug, Clone, Default)]
pub struct PropertyDescriptor {
    pub value: Option<Value>,
    pub writable: Option<bool>,
    pub get: Option<Value>,
    pub set: Option<Value>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}

impl PropertyDescriptor {
    pub fn data(value: Value, writable: bool, enumerable: bool, configurable: bool) -> Self {
        Self {
            value: Some(value),
            writable: Some(writable),
            enumerable: Some(enumerable),
            configurable: Some(configurable),
            ..Self::default()
        }
    }
    pub(crate) fn accessor(&self) -> bool {
        self.get.is_some() || self.set.is_some()
    }
}
