// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Map` and `Set` iteration: `keys`/`values`/`entries`, `forEach`, and the
//! `%MapIteratorPrototype%`/`%SetIteratorPrototype%` objects their iterators
//! inherit from (ECMA-262 24.1.5, 24.2.5, 24.3.3, 24.4.3).
//!
//! The entry list and its live-iteration behaviour are the heap's
//! (`heap/collection_iteration.rs`); this module is the JavaScript-visible layer
//! -- receiver brand checks, the callback protocol, iterator result records.

use super::*;
use crate::heap::CollectionEntry;

impl Vm {
    /// `%MapIteratorPrototype%` (`map`) or `%SetIteratorPrototype%`: the
    /// `next` method and the `@@toStringTag`, inheriting from
    /// `%IteratorPrototype%` (which makes an iterator iterable).
    pub(in super::super) fn collection_iterator_prototype(
        &mut self,
        map: bool,
    ) -> Result<ObjectId, RuntimeError> {
        let cached = if map {
            self.map_iterator_prototype
        } else {
            self.set_iterator_prototype
        };
        if let Some(prototype) = cached {
            return Ok(prototype);
        }
        let base = self.base_iterator_prototype()?;
        let function_prototype = self.function_prototype()?;
        let prototype = self.with_roots(|heap| heap.alloc_object(Some(base)))?;
        let root = self.heap.root(prototype)?;
        let result = (|| {
            self.install_native(
                prototype,
                function_prototype,
                "next",
                0,
                NativeFunction::CollectionIteratorNext { map },
            )?;
            self.define_data(
                prototype,
                JsSymbol::well_known("toStringTag"),
                Value::String(if map { "Map Iterator" } else { "Set Iterator" }.into()),
                false,
                false,
                true,
            )?;
            Ok(prototype)
        })();
        match result {
            Ok(prototype) => {
                if map {
                    self.map_iterator_prototype = Some(prototype);
                } else {
                    self.set_iterator_prototype = Some(prototype);
                }
                Ok(prototype)
            }
            Err(error) => {
                self.heap.unroot(root)?;
                Err(error)
            }
        }
    }

    /// `CreateMapIterator` / `CreateSetIterator` over the already
    /// brand-checked `collection`.
    pub(in super::super) fn collection_iterator(
        &mut self,
        collection: ObjectId,
        map: bool,
        kind: ArrayIteratorKind,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.collection_iterator_prototype(map)?;
        Ok(Value::Object(self.with_roots(|heap| {
            heap.alloc_collection_iterator(collection, map, kind, prototype)
        })?))
    }

    /// `%MapIteratorPrototype%.next` / `%SetIteratorPrototype%.next`.
    pub(in super::super) fn collection_iterator_next(
        &mut self,
        map: bool,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let step = match receiver.object_id() {
            Some(iterator) => self.heap.collection_iterator_next(iterator, map)?,
            None => None,
        };
        let Some(step) = step else {
            return Err(RuntimeError::TypeError(format!(
                "{} iterator next requires a {} iterator",
                if map { "Map" } else { "Set" },
                if map { "Map" } else { "Set" },
            )));
        };
        let Some((key, value, kind)) = step else {
            return self.iterator_result(Value::Undefined, true);
        };
        // A Set stores its members as keys; its "value" side is the member
        // itself, so `values()` and `entries()` (`[member, member]`) read
        // right without a special case in the caller.
        let (first, second) = if map {
            (key, value)
        } else {
            (key.clone(), key)
        };
        let produced = match kind {
            ArrayIteratorKind::Keys => first,
            ArrayIteratorKind::Values => second,
            ArrayIteratorKind::Entries => self.array_from(vec![first, second])?,
        };
        self.iterator_result(produced, false)
    }

    /// `Map.prototype.forEach` / `Set.prototype.forEach` over the already
    /// brand-checked `collection`: the callback is required to be callable
    /// *before* any iteration, is invoked as `callback(value, key, collection)`
    /// (a Set passes the member as both), and sees every change the callback
    /// itself makes -- new entries are reached, deleted ones skipped.
    pub(in super::super) fn collection_for_each(
        &mut self,
        collection: ObjectId,
        map: bool,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let callback = native::argument(args, 0).clone();
        if !self.is_callable(&callback)? {
            return Err(RuntimeError::TypeError(
                "forEach requires a callable callback".into(),
            ));
        }
        let this_arg = native::argument(args, 1).clone();
        let base = self.stack.len();
        self.stack.push(callback.clone());
        self.stack.push(this_arg.clone());
        let result = (|| {
            let mut index = 0;
            loop {
                match self.heap.collection_entry_at(collection, index)? {
                    CollectionEntry::End => return Ok(Value::Undefined),
                    CollectionEntry::Deleted => index += 1,
                    CollectionEntry::Present(key, value) => {
                        index += 1;
                        let (value, key) = if map {
                            (value, key)
                        } else {
                            (key.clone(), key)
                        };
                        let entry_base = self.stack.len();
                        self.stack.push(value.clone());
                        self.stack.push(key.clone());
                        self.call_native(
                            callback.clone(),
                            this_arg.clone(),
                            vec![value, key, Value::Object(collection)],
                            false,
                        )?;
                        self.stack.truncate(entry_base);
                    }
                }
            }
        })();
        self.stack.truncate(base);
        result
    }
}
