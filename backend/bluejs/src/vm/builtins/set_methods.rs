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
                _ => unreachable!("not a set-algebra method"),
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
        if !self.is_callable(&has)? {
            return Err(RuntimeError::TypeError(
                "Set method argument has no callable has".into(),
            ));
        }
        self.stack.push(has.clone());
        let keys = self.get_property(other, &"keys".into())?;
        if !self.is_callable(&keys)? {
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
        let prototype = self.collection_prototype(false)?;
        let result = self.with_roots(|heap| heap.alloc_set(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        if copy {
            for member in self.set_members(set)? {
                self.with_roots(|heap| heap.set_add(result, member))?;
            }
        }
        Ok(result)
    }

    /// The live members of `set` in insertion order.
    fn set_members(&mut self, set: ObjectId) -> Result<Vec<Value>, RuntimeError> {
        let mut members = Vec::new();
        let mut index = 0;
        loop {
            match self.heap.collection_entry_at(set, index)? {
                CollectionEntry::End => return Ok(members),
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
            match self.heap.collection_entry_at(set, index)? {
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
        if (self.heap.set_size(set)? as f64) <= record.size {
            self.set_walk_live(set, |vm, member| {
                if vm.set_record_has(record, &member)? && !vm.heap.set_has(result, &member)? {
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
                    if self.heap.set_has(set, &next)? && !self.heap.set_has(result, &next)? {
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
        if (self.heap.set_size(set)? as f64) <= record.size {
            // The copy fixes which elements are probed, whatever `has`
            // does to the receiver meanwhile.
            for member in self.set_members(set)? {
                let base = self.stack.len();
                self.stack.push(member.clone());
                let outcome: Result<(), RuntimeError> = (|| {
                    if self.set_record_has(record, &member)? {
                        self.heap.set_delete(result, &member)?;
                    }
                    Ok(())
                })();
                self.stack.truncate(base);
                outcome?;
            }
        } else {
            let keys = self.set_record_keys(record)?;
            while let Some(next) = self.iterator_step(&keys, true)? {
                self.heap.set_delete(result, &next)?;
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
                let already_in_result = self.heap.set_has(result, &next)?;
                if self.heap.set_has(set, &next)? {
                    if already_in_result {
                        self.heap.set_delete(result, &next)?;
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
        if (self.heap.set_size(set)? as f64) > record.size {
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
        if (self.heap.set_size(set)? as f64) < record.size {
            return Ok(Value::Bool(false));
        }
        let keys = self.set_record_keys(record)?;
        while let Some(next) = self.iterator_step(&keys, true)? {
            if !self.heap.set_has(set, &next)? {
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
        if (self.heap.set_size(set)? as f64) <= record.size {
            let shared = self.set_walk_live(set, |vm, member| {
                Ok(vm.set_record_has(record, &member)?.then_some(()))
            })?;
            return Ok(Value::Bool(shared.is_none()));
        }
        let keys = self.set_record_keys(record)?;
        while let Some(next) = self.iterator_step(&keys, true)? {
            if self.heap.set_has(set, &next)? {
                self.iterator_close(&keys)?;
                return Ok(Value::Bool(false));
            }
        }
        Ok(Value::Bool(true))
    }
}
