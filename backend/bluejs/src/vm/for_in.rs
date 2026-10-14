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
        // Check heap ingress before relying on compiler-owned record fields.
        self.heap.get_own(record, "forInVisited")?;
        let base = self.stack.len();
        self.stack.push(Value::Object(record));
        let result = self.for_in_next_key(record);
        if !matches!(result, Ok(Some(_))) {
            self.heap
                .set(record, "done", Value::Bool(true))
                .expect("the retained private record owns its existing Boolean done slot");
        }
        self.stack.truncate(base);
        result
    }

    fn for_in_next_key(&mut self, record: ObjectId) -> Result<Option<Value>, RuntimeError> {
        // No script can access or mutate these ordinary metadata objects.
        // The record retains them across every observable Proxy operation.
        let visited = self
            .heap
            .get_own(record, "forInVisited")
            .expect("for_in_step validated the retained private record")
            .and_then(|value| value.object_id())
            .expect("for_in_iterator installs the private visited object");
        loop {
            let Some(Value::Object(object)) = self
                .heap
                .get_own(record, "forInObject")
                .expect("the private record is retained")
            else {
                return Ok(None);
            };
            self.stack.push(Value::Object(object));
            let keys = match self
                .heap
                .get_own(record, "forInKeys")
                .expect("the private record is retained")
            {
                Some(Value::Object(keys)) => keys,
                _ => {
                    // Entering this object: guard against a cyclic chain
                    // (a Proxy can return one), then list its own keys.
                    let chain = self
                        .heap
                        .get_own(record, "forInChain")
                        .expect("the private record is retained")
                        .and_then(|value| value.object_id())
                        .expect("for_in_iterator installs the private chain array");
                    if self
                        .array_contains_object(chain, object)
                        .expect("the record retains its private chain array")
                    {
                        self.heap
                            .set(record, "forInObject", Value::Null)
                            .expect("replacing a private non-payload slot does not grow storage");
                        continue;
                    }
                    let length = self
                        .heap
                        .get_own(chain, "length")
                        .expect("the private metadata is retained")
                        .and_then(|value| value.as_number())
                        .expect("private arrays and indices have numeric lengths")
                        as usize;
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
                    let keys = keys
                        .object_id()
                        .expect("array_from returns an array object");
                    self.stack.push(Value::Object(keys));
                    self.heap
                        .set(record, "forInKeys", Value::Object(keys))
                        .expect("replacing a private non-payload slot does not grow storage");
                    self.heap
                        .set(record, "forInIndex", Value::Number(0.0))
                        .expect("replacing a private non-payload slot does not grow storage");
                    keys
                }
            };
            let length = self
                .heap
                .get_own(keys, "length")
                .expect("the private metadata is retained")
                .and_then(|value| value.as_number())
                .expect("private arrays and indices have numeric lengths")
                as usize;
            let mut index = self
                .heap
                .get_own(record, "forInIndex")
                .expect("the private metadata is retained")
                .and_then(|value| value.as_number())
                .expect("private arrays and indices have numeric lengths")
                as usize;
            while index < length {
                let key = self
                    .heap
                    .get_own(keys, index.to_string())
                    .expect("the record retains the dense private key array")
                    .and_then(|value| value.as_string().cloned())
                    .expect("the captured keys contain only string names");
                index += 1;
                self.heap
                    .set(record, "forInIndex", Value::Number(index as f64))
                    .expect("replacing the private numeric index does not grow storage");
                let name = PropertyName::String(key.clone());
                if self
                    .heap
                    .get_own(visited, name.clone())
                    .expect("the record retains its ordinary visited object")
                    .is_some()
                {
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
            self.heap
                .set(record, "forInObject", next)
                .expect("replacing a private non-payload slot does not grow storage");
            self.heap
                .set(record, "forInKeys", Value::Undefined)
                .expect("replacing a private non-payload slot does not grow storage");
        }
    }

    fn array_contains_object(
        &mut self,
        array: ObjectId,
        object: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let length =
            self.heap
                .get_own(array, "length")
                .expect("the private metadata is retained")
                .and_then(|value| value.as_number())
                .expect("private arrays and indices have numeric lengths") as usize;
        for index in 0..length {
            if self
                .heap
                .get_own(array, index.to_string())
                .expect("the record retains its private chain array")
                == Some(Value::Object(object))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
