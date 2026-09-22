// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Index walks over huge sparse array-likes.
//!
//! `Array.prototype` methods probe every index below `length` with
//! `[[HasProperty]]`. When neither the receiver nor anything on its prototype
//! chain can materialise an index without storing it (no Proxy, String,
//! TypedArray or namespace object), an index that no object on the chain
//! stores cannot be found, and probing it is unobservable. `IndexScan` lets a
//! walk jump between the stored indices instead of paying one step per hole,
//! while the walk keeps performing the specification's own probe on every
//! index it does visit.
//!
//! User code (a getter, a callback) may add or remove properties or swap a
//! prototype mid-walk, so the stored-index list is rebuilt whenever the heap's
//! structure epoch moved. Once rebuilding costs more than a plain walk would,
//! the scan falls back to visiting every index.
use super::*;

/// A plain walk is cheaper than building a stored-index list below this length.
const SPARSE_LENGTH_THRESHOLD: u64 = 65_536;

pub(super) struct IndexScan {
    length: u64,
    /// Ascending indices below `length` that some object on the chain stores,
    /// or `None` to visit every index.
    stored: Option<Vec<u64>>,
    epoch: u64,
    /// Property keys examined by all rebuilds so far.
    work: u64,
}

impl Vm {
    /// `LengthOfArrayLike(object)`.
    pub(super) fn array_like_length(&mut self, object: ObjectId) -> Result<u64, RuntimeError> {
        let length = self.get_property(&Value::Object(object), &"length".into())?;
        Ok(self.coerce_length(&length)? as u64)
    }

    pub(super) fn index_scan(
        &mut self,
        object: ObjectId,
        length: u64,
    ) -> Result<IndexScan, RuntimeError> {
        let mut scan = IndexScan {
            length,
            stored: None,
            epoch: 0,
            work: 0,
        };
        if length >= SPARSE_LENGTH_THRESHOLD {
            self.rebuild_index_scan(&mut scan, object)?;
        }
        Ok(scan)
    }

    fn rebuild_index_scan(
        &mut self,
        scan: &mut IndexScan,
        object: ObjectId,
    ) -> Result<(), RuntimeError> {
        scan.epoch = self.heap.structure_epoch();
        scan.stored = None;
        let mut stored = Vec::new();
        let mut current = Some(object);
        while let Some(id) = current {
            let Some(keys) = self.heap.own_integer_keys(id)? else {
                return Ok(());
            };
            scan.work += keys.len() as u64 + 1;
            stored.extend(keys.into_iter().take_while(|key| *key < scan.length));
            current = self.heap.prototype(id)?;
        }
        if scan.work <= scan.length / 4 {
            stored.sort_unstable();
            stored.dedup();
            scan.stored = Some(stored);
        }
        Ok(())
    }

    fn refresh_index_scan(
        &mut self,
        scan: &mut IndexScan,
        object: ObjectId,
    ) -> Result<(), RuntimeError> {
        if scan.stored.is_some() && scan.epoch != self.heap.structure_epoch() {
            self.rebuild_index_scan(scan, object)?;
        }
        Ok(())
    }

    /// The smallest index in `[from, length)` a walk must probe, or `None` once
    /// every remaining index is provably absent. A returned index greater than
    /// `from`, or `None` with `from < length`, means the indices in between
    /// read as `undefined`.
    pub(super) fn scan_next(
        &mut self,
        scan: &mut IndexScan,
        object: ObjectId,
        from: u64,
    ) -> Result<Option<u64>, RuntimeError> {
        self.refresh_index_scan(scan, object)?;
        Ok(match &scan.stored {
            Some(stored) => stored
                .get(stored.partition_point(|index| *index < from))
                .copied(),
            None => (from < scan.length).then_some(from),
        })
    }

    /// The largest index below `end` a walk must probe.
    fn scan_previous(
        &mut self,
        scan: &mut IndexScan,
        object: ObjectId,
        end: u64,
    ) -> Result<Option<u64>, RuntimeError> {
        self.refresh_index_scan(scan, object)?;
        Ok(match &scan.stored {
            Some(stored) => stored[..stored.partition_point(|index| *index < end)]
                .last()
                .copied(),
            None => end.checked_sub(1),
        })
    }

    /// The next index at or after `from` where a walk finds a property, as
    /// `(index, value)`. The caller keeps `object` and the value reachable.
    pub(super) fn array_next_present(
        &mut self,
        scan: &mut IndexScan,
        object: ObjectId,
        from: u64,
    ) -> Result<Option<(u64, Value)>, RuntimeError> {
        let mut from = from;
        while let Some(index) = self.scan_next(scan, object, from)? {
            if let Some(value) = self.array_probe(object, index)? {
                return Ok(Some((index, value)));
            }
            from = index + 1;
        }
        Ok(None)
    }

    /// The next index below `end`, walking downwards, where a walk finds a
    /// property, as `(index, value)`.
    pub(super) fn array_previous_present(
        &mut self,
        scan: &mut IndexScan,
        object: ObjectId,
        end: u64,
    ) -> Result<Option<(u64, Value)>, RuntimeError> {
        let mut end = end;
        while let Some(index) = self.scan_previous(scan, object, end)? {
            if let Some(value) = self.array_probe(object, index)? {
                return Ok(Some((index, value)));
            }
            end = index;
        }
        Ok(None)
    }

    /// `HasProperty` then `Get` for one index.
    fn array_probe(&mut self, object: ObjectId, index: u64) -> Result<Option<Value>, RuntimeError> {
        self.charge_step()?;
        let key: PropertyName = index.to_string().into();
        if !self.has_property(object, &key)? {
            return Ok(None);
        }
        self.get_property(&Value::Object(object), &key).map(Some)
    }
}
