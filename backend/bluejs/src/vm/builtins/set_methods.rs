// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `Set.prototype` set-algebra methods (ECMA-262 24.2.4.5-24.2.4.14 in
//! the ES2025 numbering): `union`, `intersection`, `difference`,
//! `symmetricDifference`, `isSubsetOf`, `isSupersetOf` and `isDisjointFrom`.
//!
//! Each takes a *set-like* argument that is read exactly once through
//! GetSetRecord (`size`, then `has`, then `keys`) and afterwards only ever
//! used through the captured `has`/`keys` functions. The receiver's entries
//! are walked live through the heap's tombstoned entry list
//! (`heap/collection_iteration.rs`), which is what makes "the receiver grew or
//! lost elements while `has` ran" behave as the specification's index loop
//! does.

use super::*;
use crate::heap::CollectionEntry;

/// The Set Record of GetSetRecord: the set-like object and the three things
/// read from it once, up front.
struct SetRecord {
    object: Value,
    size: f64,
    has: Value,
    keys: Value,
}

impl Vm {
    /// Dispatch for the seven algebra methods. `set` is the already
    /// brand-checked receiver.
    pub(in super::super) fn set_algebra_method(
        &mut self,
        method: SetMethod,
        set: ObjectId,
        other: &Value,
    ) -> Result<Value, RuntimeError> {
        let base = self.stack.len();
        self.stack.extend([Value::Object(set), other.clone()]);
        let result = (|| {
            let record = self.get_set_record(other)?;
            self.stack.extend([record.has.clone(), record.keys.clone()]);
            match method {
                SetMethod::Union => self.set_union(set, &record),
                SetMethod::Intersection => self.set_intersection(set, &record),
                SetMethod::Difference => self.set_difference(set, &record),
                SetMethod::SymmetricDifference => self.set_symmetric_difference(set, &record),
                SetMethod::IsSubsetOf => self.set_is_subset_of(set, &record),
                SetMethod::IsSupersetOf => self.set_is_superset_of(set, &record),
                SetMethod::IsDisjointFrom => self.set_is_disjoint_from(set, &record),
                _ => Err(RuntimeError::TypeError("not a set-algebra method".into())),
            }
        })();
        self.stack.truncate(base);
        result
    }

    /// GetSetRecord ( obj ).
    fn get_set_record(&mut self, other: &Value) -> Result<SetRecord, RuntimeError> {
        if !matches!(other, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Set method argument must be an object".into(),
            ));
        }
        let raw_size = self.get_property(other, &"size".into())?;
        let number = self.coerce_number(&raw_size)?;
        if number.is_nan() {
            return Err(RuntimeError::TypeError(
                "Set method argument has no numeric size".into(),
            ));
        }
        // ToIntegerOrInfinity; a size in (-1, 0) truncates to zero and passes.
        let size = number.trunc();
        if size < 0.0 {
            return Err(RuntimeError::RangeError(
                "Set method argument has a negative size".into(),
            ));
        }
        let has = self.get_property(other, &"has".into())?;
        if !self.callable(&has) {
            return Err(RuntimeError::TypeError(
                "Set method argument has no callable has".into(),
            ));
        }
        self.stack.push(has.clone());
        let keys = self.get_property(other, &"keys".into())?;
        if !self.callable(&keys) {
            return Err(RuntimeError::TypeError(
                "Set method argument has no callable keys".into(),
            ));
        }
        Ok(SetRecord {
            object: other.clone(),
            size,
            has,
            keys,
        })
    }

    /// ToBoolean(? Call(setRec.[[Has]], setRec.[[SetObject]], « value »)).
    fn set_record_has(&mut self, record: &SetRecord, value: &Value) -> Result<bool, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(value.clone());
        let result = self
            .call_native(
                record.has.clone(),
                record.object.clone(),
                vec![value.clone()],
                false,
            )
            .and_then(|result| self.to_boolean(&result));
        self.stack.truncate(base);
        result
    }

    /// GetIteratorFromMethod(setRec.[[SetObject]], setRec.[[Keys]]); the
    /// returned iterator record is pushed on the stack for the caller to
    /// truncate away.
    fn set_record_keys(&mut self, record: &SetRecord) -> Result<Value, RuntimeError> {
        let iterator = self.get_iterator_from_method(&record.object, record.keys.clone())?;
        self.stack.push(iterator.clone());
        Ok(iterator)
    }

    /// A new `%Set.prototype%` object holding the live entries of `set`
    /// (`copy`) or nothing, already pushed on the stack.
    fn set_algebra_result(&mut self, set: ObjectId, copy: bool) -> Result<ObjectId, RuntimeError> {
        // `set` exists, and the Set constructor materializes `%Set.prototype%`
        // before it allocates one, so this lookup only reads the cache.
        let prototype = self
            .collection_prototype(false)
            .expect("%Set.prototype% is materialized once a Set exists");
        let result = self.with_roots(|heap| heap.alloc_set(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        if copy {
            for member in self.set_members(set) {
                self.with_roots(|heap| heap.set_add(result, member))?;
            }
        }
        Ok(result)
    }

    // The receiver and every result set are live Set objects, so the heap
    // lookups below can only fail on a dangling handle.
    const SET_LIVE: &'static str = "a receiver or result Set is a live Set object";

    fn set_entry(&self, set: ObjectId, index: usize) -> CollectionEntry {
        self.heap
            .collection_entry_at(set, index)
            .expect(Self::SET_LIVE)
    }

    fn set_len(&self, set: ObjectId) -> f64 {
        self.heap.set_size(set).expect(Self::SET_LIVE) as f64
    }

    fn set_contains(&self, set: ObjectId, member: &Value) -> bool {
        self.heap.set_has(set, member).expect(Self::SET_LIVE)
    }

    fn set_remove(&mut self, set: ObjectId, member: &Value) {
        self.heap.set_delete(set, member).expect(Self::SET_LIVE);
    }

    /// The live members of `set` in insertion order.
    fn set_members(&self, set: ObjectId) -> Vec<Value> {
        let mut members = Vec::new();
        let mut index = 0;
        loop {
            match self.set_entry(set, index) {
                CollectionEntry::End => return members,
                CollectionEntry::Deleted => {}
                CollectionEntry::Present(member, _) => members.push(member),
            }
            index += 1;
        }
    }

    /// Runs `visit` over `set`'s entries at the moment each position is
    /// reached (so entries appended by `visit` are visited and entries it
    /// deleted are skipped), stopping early when `visit` returns `Some`.
    fn set_walk_live<T>(
        &mut self,
        set: ObjectId,
        mut visit: impl FnMut(&mut Self, Value) -> Result<Option<T>, RuntimeError>,
    ) -> Result<Option<T>, RuntimeError> {
        let mut index = 0;
        loop {
            match self.set_entry(set, index) {
                CollectionEntry::End => return Ok(None),
                CollectionEntry::Deleted => {}
                CollectionEntry::Present(member, _) => {
                    let base = self.stack.len();
                    self.stack.push(member.clone());
                    let step = visit(self, member);
                    self.stack.truncate(base);
                    if let Some(outcome) = step? {
                        return Ok(Some(outcome));
                    }
                }
            }
            index += 1;
        }
    }

    fn set_union(&mut self, set: ObjectId, record: &SetRecord) -> Result<Value, RuntimeError> {
        let keys = self.set_record_keys(record)?;
        let result = self.set_algebra_result(set, true)?;
        while let Some(next) = self.iterator_step(&keys, true)? {
            let base = self.stack.len();
            self.stack.push(next.clone());
            let added = self.with_roots(|heap| heap.set_add(result, next));
            self.stack.truncate(base);
            added?;
        }
        Ok(Value::Object(result))
    }

    fn set_intersection(
        &mut self,
        set: ObjectId,
        record: &SetRecord,
    ) -> Result<Value, RuntimeError> {
        let result = self.set_algebra_result(set, false)?;
        if self.set_len(set) <= record.size {
            self.set_walk_live(set, |vm, member| {
                if vm.set_record_has(record, &member)? && !vm.set_contains(result, &member) {
                    vm.with_roots(|heap| heap.set_add(result, member))?;
                }
                Ok(None::<()>)
            })?;
        } else {
            let keys = self.set_record_keys(record)?;
            while let Some(next) = self.iterator_step(&keys, true)? {
                let base = self.stack.len();
                self.stack.push(next.clone());
                let outcome: Result<(), RuntimeError> = (|| {
                    if self.set_contains(set, &next) && !self.set_contains(result, &next) {
                        self.with_roots(|heap| heap.set_add(result, next))?;
                    }
                    Ok(())
                })();
                self.stack.truncate(base);
                outcome?;
            }
        }
        Ok(Value::Object(result))
    }

    fn set_difference(&mut self, set: ObjectId, record: &SetRecord) -> Result<Value, RuntimeError> {
        let result = self.set_algebra_result(set, true)?;
        if self.set_len(set) <= record.size {
            // The copy fixes which elements are probed, whatever `has`
            // does to the receiver meanwhile.
            for member in self.set_members(set) {
                let base = self.stack.len();
                self.stack.push(member.clone());
                let outcome: Result<(), RuntimeError> = (|| {
                    if self.set_record_has(record, &member)? {
                        self.set_remove(result, &member);
                    }
                    Ok(())
                })();
                self.stack.truncate(base);
                outcome?;
            }
        } else {
            let keys = self.set_record_keys(record)?;
            while let Some(next) = self.iterator_step(&keys, true)? {
                self.set_remove(result, &next);
            }
        }
        Ok(Value::Object(result))
    }

    fn set_symmetric_difference(
        &mut self,
        set: ObjectId,
        record: &SetRecord,
    ) -> Result<Value, RuntimeError> {
        let keys = self.set_record_keys(record)?;
        let result = self.set_algebra_result(set, true)?;
        while let Some(next) = self.iterator_step(&keys, true)? {
            let base = self.stack.len();
            self.stack.push(next.clone());
            let outcome: Result<(), RuntimeError> = (|| {
                let already_in_result = self.set_contains(result, &next);
                if self.set_contains(set, &next) {
                    if already_in_result {
                        self.set_remove(result, &next);
                    }
                } else if !already_in_result {
                    self.with_roots(|heap| heap.set_add(result, next))?;
                }
                Ok(())
            })();
            self.stack.truncate(base);
            outcome?;
        }
        Ok(Value::Object(result))
    }

    fn set_is_subset_of(
        &mut self,
        set: ObjectId,
        record: &SetRecord,
    ) -> Result<Value, RuntimeError> {
        if self.set_len(set) > record.size {
            return Ok(Value::Bool(false));
        }
        let missing = self.set_walk_live(set, |vm, member| {
            Ok((!vm.set_record_has(record, &member)?).then_some(()))
        })?;
        Ok(Value::Bool(missing.is_none()))
    }

    fn set_is_superset_of(
        &mut self,
        set: ObjectId,
        record: &SetRecord,
    ) -> Result<Value, RuntimeError> {
        if self.set_len(set) < record.size {
            return Ok(Value::Bool(false));
        }
        let keys = self.set_record_keys(record)?;
        while let Some(next) = self.iterator_step(&keys, true)? {
            if !self.set_contains(set, &next) {
                self.iterator_close(&keys)?;
                return Ok(Value::Bool(false));
            }
        }
        Ok(Value::Bool(true))
    }

    fn set_is_disjoint_from(
        &mut self,
        set: ObjectId,
        record: &SetRecord,
    ) -> Result<Value, RuntimeError> {
        if self.set_len(set) <= record.size {
            let shared = self.set_walk_live(set, |vm, member| {
                Ok(vm.set_record_has(record, &member)?.then_some(()))
            })?;
            return Ok(Value::Bool(shared.is_none()));
        }
        let keys = self.set_record_keys(record)?;
        while let Some(next) = self.iterator_step(&keys, true)? {
            if self.set_contains(set, &next) {
                self.iterator_close(&keys)?;
                return Ok(Value::Bool(false));
            }
        }
        Ok(Value::Bool(true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile, parse};

    #[test]
    fn only_the_seven_algebra_methods_are_dispatched() {
        let mut vm = Vm::default();
        let pair = vm
            .execute(
                &compile(
                    &parse(
                        "[new Set([1]), { size: 0, has() { return false; }, keys() { return [].values(); } }]",
                    )
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap()
            .object_id()
            .unwrap();
        let set = vm.heap().get(pair, "0").unwrap().object_id().unwrap();
        let other = vm.heap().get(pair, "1").unwrap();
        assert_eq!(
            vm.set_algebra_method(SetMethod::Add, set, &other),
            Err(RuntimeError::TypeError("not a set-algebra method".into()))
        );
        assert_eq!(
            vm.set_algebra_method(SetMethod::IsDisjointFrom, set, &other),
            Ok(Value::Bool(true))
        );
        for method in [
            SetMethod::Union,
            SetMethod::Intersection,
            SetMethod::Difference,
            SetMethod::SymmetricDifference,
            SetMethod::IsSubsetOf,
            SetMethod::IsSupersetOf,
            SetMethod::IsDisjointFrom,
        ] {
            assert!(vm.set_algebra_method(method, set, &other).is_ok());
            // A set-like operand that is not an object fails before any work.
            assert!(vm
                .set_algebra_method(method, set, &Value::Undefined)
                .is_err());
        }
    }
}
