// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The map from a binding's slot number to the cell that holds it.
//!
//! Keys are small integers the compiler assigned, not input an attacker can choose a
//! collision for, so the default SipHash (a keyed hash built to resist exactly that) is pure
//! cost here: every closure call builds one of these from its captures, and in a profile of
//! a script-heavy page that was a fifth of all the time. A multiplicative hash of the key
//! does the job; hashbrown reads the low bits for the bucket and the high bits for its
//! control byte, and multiplying by an odd constant keeps the low bits of consecutive
//! slots distinct and spreads the high ones.

use crate::value::ObjectId;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default, Clone, Copy)]
pub(crate) struct SlotHasher(u64);

impl Hasher for SlotHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        // Only integers are hashed with this; fold anything else in rather than ignore it.
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_usize(&mut self, n: usize) {
        self.0 = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

pub(crate) type CellMap = HashMap<usize, ObjectId, BuildHasherDefault<SlotHasher>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_slots_land_in_distinct_buckets_and_round_trip() {
        let mut map = CellMap::default();
        for slot in 0..1000usize {
            map.insert(
                slot,
                ObjectId {
                    heap: 1,
                    serial: slot as u64,
                },
            );
        }
        assert_eq!(map.len(), 1000);
        for slot in 0..1000usize {
            assert_eq!(
                map[&slot],
                ObjectId {
                    heap: 1,
                    serial: slot as u64
                }
            );
        }
        // Low bits of the hash, which pick the bucket, are distinct for a dense run.
        let mut seen = std::collections::HashSet::new();
        for slot in 0..1024usize {
            let mut h = SlotHasher::default();
            h.write_usize(slot);
            assert!(seen.insert(h.finish() & 1023), "slot {slot} collides");
        }
    }

    #[test]
    fn a_hash_of_other_bytes_still_depends_on_them() {
        let hash = |bytes: &[u8]| {
            let mut h = SlotHasher::default();
            h.write(bytes);
            h.finish()
        };
        assert_ne!(hash(b"ab"), hash(b"ba"));
        assert_ne!(hash(b"a"), hash(b"b"));
    }
}
