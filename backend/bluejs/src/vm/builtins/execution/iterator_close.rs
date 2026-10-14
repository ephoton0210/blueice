// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Await-capable for-await cleanup, preserving the incoming completion.
//! https://tc39.es/ecma262/#sec-asynciteratorclose
//! https://tc39.es/ecma262/#sec-async-from-sync-iterator-prototype-return

use super::*;

impl Vm {
    pub(in super::super::super) fn async_iterator_close(
        &mut self,
        record: &Value,
        throwing: bool,
    ) -> Result<Option<Value>, RuntimeError> {
        let id = record
            .object_id()
            .expect("compiler retains its iterator record");
        let done = self.heap.get_own(id, "done")? == Some(Value::Bool(true));
        let next_pending = self.heap.get_own(id, "asyncNextPending")? == Some(Value::Bool(true));
        self.heap.set(id, "done", Value::Bool(true))?;
        self.with_roots(|heap| heap.set(id, "asyncClosePending", Value::Bool(false)))?;
        if done || next_pending {
            return Ok(None);
        }
        let base = self.stack.len();
        self.stack.push(record.clone());
        let result = (|| {
            let iterator = self
                .heap
                .get_own(id, "iterator")?
                .unwrap_or(Value::Undefined);
            self.stack.push(iterator.clone());
            let from_sync = self.heap.get_own(id, "asyncFromSync")? == Some(Value::Bool(true));
            let input = if from_sync {
                let returned = (|| {
                    let method = self.get_method(&iterator, &"return".into())?;
                    if method == Value::Undefined {
                        let returned = self.iterator_result(Value::Undefined, true)?;
                        return self.promise_resolve(returned);
                    }
                    let returned = self.call_native(method, iterator.clone(), Vec::new(), false)?;
                    self.stack.push(returned.clone());
                    self.async_from_sync_continue(id, returned, false)
                })();
                match returned {
                    Ok(input) => input,
                    Err(error) => {
                        let reason = self.error_value(error)?;
                        self.promise_reject(reason)?
                    }
                }
            } else {
                let returned = (|| {
                    let method = self.get_method(&iterator, &"return".into())?;
                    if method == Value::Undefined {
                        return Ok(None);
                    }
                    self.call_native(method, iterator.clone(), Vec::new(), false)
                        .map(Some)
                })();
                match returned {
                    Ok(Some(value)) => self.promise_resolve(value)?,
                    Ok(None) => return Ok(None),
                    Err(error) if throwing && error.is_catchable() => return Ok(None),
                    Err(error) => return Err(error),
                }
            };
            let input = if from_sync {
                self.promise_resolve(input)?
            } else {
                input
            };
            self.stack.push(input.clone());
            self.with_roots(|heap| heap.set(id, "asyncClosePending", Value::Bool(true)))?;
            Ok(Some(input))
        })();
        self.stack.truncate(base);
        match result {
            Err(error) if throwing && error.is_catchable() => Ok(None),
            result => result,
        }
    }
}
