// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Internal for-in records and observable subject enumeration.

use super::*;

impl Vm {
    /// Snapshots the enumerable string keys visible through an object's
    /// prototype chain. Non-enumerable own keys still suppress an inherited
    /// key with the same name; symbols never participate in `for-in`.
    /// The iterator record of a `for-in` loop (EnumerateObjectProperties,
    /// §14.7.5.9). It is an engine-internal record, not an ECMAScript
    /// iterator: `for_in_step` walks the object and its prototype chain lazily,
    /// so a key deleted (or made non-enumerable) before it is visited is not
    /// produced, and the loop never touches user-visible iterator methods.
    pub(super) fn for_in_iterator(&mut self, source: &Value) -> Result<Value, RuntimeError> {
        // ForIn/OfHeadEvaluation: a `null` or `undefined` subject enumerates
        // nothing instead of failing ToObject.
        let object = if matches!(source, Value::Null | Value::Undefined) {
            Value::Null
        } else {
            Value::Object(self.coerce_object(source)?)
        };
        let base = self.stack.len();
        // The coerced object of a primitive subject is new: keep it reachable
        // while the record's own objects are allocated.
        self.stack.push(object.clone());
        let result = (|| {
            let record = self.with_roots(|heap| heap.alloc_object(None))?;
            self.stack.push(Value::Object(record));
            let visited = self.with_roots(|heap| heap.alloc_object(None))?;
            self.stack.push(Value::Object(visited));
            let chain = self.array_from(Vec::new())?;
            self.stack.push(chain.clone());
            self.with_roots(|heap| heap.set(record, "forInObject", object))?;
            self.with_roots(|heap| heap.set(record, "forInKeys", Value::Undefined))?;
            self.with_roots(|heap| heap.set(record, "forInIndex", Value::Number(0.0)))?;
            self.with_roots(|heap| heap.set(record, "forInVisited", Value::Object(visited)))?;
            self.with_roots(|heap| heap.set(record, "forInChain", chain))?;
            self.with_roots(|heap| heap.set(record, "done", Value::Bool(false)))?;
            Ok(Value::Object(record))
        })();
        self.stack.truncate(base);
        result
    }

    /// Whether `record` is a `for-in` record made by [`Vm::for_in_iterator`].
    pub(super) fn is_for_in_record(&self, record: ObjectId) -> Result<bool, RuntimeError> {
        Ok(self.heap.get_own(record, "forInObject")?.is_some())
    }

    /// The next key of a `for-in` loop, or `None` once every object of the
    /// prototype chain is exhausted. Each own key is checked with
    /// [[GetOwnProperty]] when it is about to be produced: a key that is gone
    /// or no longer enumerable is skipped, and one seen on an object nearer
    /// the receiver (enumerable or not) shadows the same key further up.
    pub(super) fn for_in_step(&mut self, record: ObjectId) -> Result<Option<Value>, RuntimeError> {
        let base = self.stack.len();
        self.stack.push(Value::Object(record));
        let result = self.for_in_next_key(record);
        self.stack.truncate(base);
        if !matches!(result, Ok(Some(_))) {
            self.with_roots(|heap| heap.set(record, "done", Value::Bool(true)))?;
        }
        result
    }

    fn for_in_next_key(&mut self, record: ObjectId) -> Result<Option<Value>, RuntimeError> {
        let visited = self.heap.get_own(record, "forInVisited")?;
        let Some(Value::Object(visited)) = visited else {
            return Ok(None);
        };
        loop {
            let Some(Value::Object(object)) = self.heap.get_own(record, "forInObject")? else {
                return Ok(None);
            };
            self.stack.push(Value::Object(object));
            let keys = match self.heap.get_own(record, "forInKeys")? {
                Some(Value::Object(keys)) => keys,
                _ => {
                    // Entering this object: guard against a cyclic chain
                    // (a Proxy can return one), then list its own keys.
                    let Some(Value::Object(chain)) = self.heap.get_own(record, "forInChain")?
                    else {
                        return Ok(None);
                    };
                    if self.array_contains_object(chain, object)? {
                        self.with_roots(|heap| heap.set(record, "forInObject", Value::Null))?;
                        continue;
                    }
                    let length = match self.heap.get_own(chain, "length")? {
                        Some(Value::Number(length)) => length as usize,
                        _ => 0,
                    };
                    self.with_roots(|heap| {
                        heap.set(chain, length.to_string(), Value::Object(object))
                    })?;
                    let mut names = Vec::new();
                    for key in self.object_own_property_keys(object)? {
                        if let PropertyName::String(key) = key {
                            names.push(Value::String(key));
                        }
                    }
                    let keys = self.array_from(names)?;
                    let Value::Object(keys) = keys else {
                        unreachable!("array_from returns an array object")
                    };
                    self.stack.push(Value::Object(keys));
                    self.with_roots(|heap| heap.set(record, "forInKeys", Value::Object(keys)))?;
                    self.with_roots(|heap| heap.set(record, "forInIndex", Value::Number(0.0)))?;
                    keys
                }
            };
            let length = match self.heap.get_own(keys, "length")? {
                Some(Value::Number(length)) => length as usize,
                _ => 0,
            };
            let mut index = match self.heap.get_own(record, "forInIndex")? {
                Some(Value::Number(index)) => index as usize,
                _ => 0,
            };
            while index < length {
                let key = self.heap.get_own(keys, index.to_string())?;
                index += 1;
                self.with_roots(|heap| {
                    heap.set(record, "forInIndex", Value::Number(index as f64))
                })?;
                let Some(Value::String(key)) = key else {
                    continue;
                };
                let name = PropertyName::String(key.clone());
                if self.heap.get_own(visited, name.clone())?.is_some() {
                    continue;
                }
                let Some(descriptor) = self.object_get_own_property(object, &name)? else {
                    continue;
                };
                self.with_roots(|heap| heap.set(visited, name, Value::Bool(true)))?;
                if descriptor.enumerable == Some(true) {
                    return Ok(Some(Value::String(key)));
                }
            }
            // This object is exhausted: continue with its prototype.
            let prototype = self.object_get_prototype(object)?;
            let next = prototype.map_or(Value::Null, Value::Object);
            self.with_roots(|heap| heap.set(record, "forInObject", next))?;
            self.with_roots(|heap| heap.set(record, "forInKeys", Value::Undefined))?;
        }
    }

    fn array_contains_object(
        &mut self,
        array: ObjectId,
        object: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let length = match self.heap.get_own(array, "length")? {
            Some(Value::Number(length)) => length as usize,
            _ => 0,
        };
        for index in 0..length {
            if self.heap.get_own(array, index.to_string())? == Some(Value::Object(object)) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
