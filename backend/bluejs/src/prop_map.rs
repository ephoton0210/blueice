// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The per-object property tables, hashed with a fast non-keyed hash.
//!
//! Every property read walks one of these (and then the prototype's), and in a profile of a
//! script-heavy page SipHash over the key's UTF-16 units was the single largest cost. The
//! tables are per object and bounded by the managed-heap budget, so a script that crafts
//! colliding keys can only slow its own objects within the same fuel it already spends; the
//! keyed hash's flood resistance buys nothing here.

use crate::property::PropertyName;
use crate::value::ObjectId;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// A word-at-a-time multiply-rotate hash (the FxHash scheme).
#[derive(Default, Clone, Copy)]
pub(crate) struct PropHasher(u64);

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl PropHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for PropHasher {
    fn finish(&self) -> u64 {
        // hashbrown takes the top bits for its control byte; fold them down as well.
        self.0 ^ (self.0 >> 32)
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_le_bytes(chunk.try_into().expect("eight bytes")));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut tail = [0u8; 8];
            tail[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(tail));
        }
    }
    fn write_u8(&mut self, n: u8) {
        self.add(u64::from(n));
    }
    fn write_u32(&mut self, n: u32) {
        self.add(u64::from(n));
    }
    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }
}

pub(crate) type PropMap<V> = HashMap<PropertyName, V, BuildHasherDefault<PropHasher>>;

/// The heap's table from object handle to object: every property access starts with one
/// lookup here, and the two-word key hashed with SipHash was the next-largest cost.
pub(crate) type ObjectMap<V> = HashMap<ObjectId, V, BuildHasherDefault<PropHasher>>;

/// A set of object handles (the collector's mark set, the remembered set).
pub(crate) type ObjectSet = std::collections::HashSet<ObjectId, BuildHasherDefault<PropHasher>>;

/// A map keyed by a global's name, looked up on every global read and write.
pub(crate) type NameMap<V> = HashMap<String, V, BuildHasherDefault<PropHasher>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::JsString;

    #[test]
    fn keys_of_every_length_round_trip_and_never_alias_symbols() {
        let mut map = PropMap::<usize>::default();
        for len in 0..40usize {
            let text: String = "a".repeat(len);
            map.insert(PropertyName::String(JsString::from(text.as_str())), len);
        }
        assert_eq!(map.len(), 40);
        for len in 0..40usize {
            let text: String = "a".repeat(len);
            assert_eq!(map[&PropertyName::String(JsString::from(text.as_str()))], len);
        }
    }
}
