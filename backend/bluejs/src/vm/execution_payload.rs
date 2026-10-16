// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Primitive buffers owned by active and displaced execution contexts.

use super::host_scheduling::{completion_payload_bytes, value_payload_bytes, values_bytes};
use super::*;
use std::cell::Cell;

pub(super) struct ExecutionPayloadCharge {
    counter: Rc<Cell<usize>>,
    bytes: usize,
}

impl ExecutionPayloadCharge {
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for ExecutionPayloadCharge {
    fn drop(&mut self) {
        self.counter.set(self.counter.get() - self.bytes);
    }
}

impl Vm {
    /// Normal dispatch moves primitive buffers to its Rust argument owner.
    /// Exact debugger continuations retain their original operand snapshots.
    pub(super) fn take_call_operand(&mut self, index: usize) -> Value {
        if self.debugger_nested_pause_request.is_some()
            || matches!(self.stack[index], Value::Object(_))
        {
            self.stack[index].clone()
        } else {
            std::mem::replace(&mut self.stack[index], Value::Undefined)
        }
    }
    pub(super) fn charge_execution_payload(&self, bytes: usize) -> ExecutionPayloadCharge {
        let bytes = if self.host_scheduling.is_some() {
            bytes
        } else {
            0
        };
        let counter = self.displaced_execution_bytes.clone();
        counter.set(counter.get().saturating_add(bytes));
        ExecutionPayloadCharge { counter, bytes }
    }

    /// Fields replaced by ordinary calls and generator entry. Operand-stack
    /// copies stay in the VM; this charge follows the displaced originals.
    pub(super) fn caller_execution_payload_bytes(&self) -> usize {
        self.bindings
            .iter()
            .flatten()
            .fold(
                values_bytes(&self.arguments).saturating_add(values_bytes(&self.with_objects)),
                |bytes, value| bytes.saturating_add(value_payload_bytes(value)),
            )
            .saturating_add(value_payload_bytes(&self.this))
            .saturating_add(value_payload_bytes(&self.callee))
            .saturating_add(value_payload_bytes(&self.completion))
            .saturating_add(value_payload_bytes(&self.new_target))
    }

    pub(super) fn pending_execution_references(&self) -> Vec<ObjectId> {
        let mut roots = Vec::new();
        for completion in &self.pending_completions {
            match completion {
                Completion::Return(value)
                | Completion::Yield(value)
                | Completion::Halt(value)
                | Completion::Throw(RuntimeError::Thrown(value)) => roots.extend(value.object_id()),
                Completion::TailCall(values) | Completion::TailRecur(values) => {
                    roots.extend(values.iter().filter_map(Value::object_id));
                }
                _ => {}
            }
        }
        roots.extend(
            self.completion_saves
                .iter()
                .filter_map(|(value, _)| value.object_id()),
        );
        roots
    }

    pub(super) fn pending_execution_payload_bytes(&self) -> usize {
        self.pending_completions
            .iter()
            .fold(0usize, |bytes, completion| {
                bytes.saturating_add(completion_payload_bytes(completion))
            })
            .saturating_add(
                self.completion_saves
                    .iter()
                    .fold(0usize, |bytes, (value, _)| {
                        bytes.saturating_add(value_payload_bytes(value))
                    }),
            )
    }

    pub(super) fn current_execution_payload_bytes(&self) -> usize {
        self.caller_execution_payload_bytes()
            .saturating_add(values_bytes(&self.stack))
            .saturating_add(self.pending_execution_payload_bytes())
    }

    pub(super) fn owned_execution_payload_bytes(&self) -> usize {
        if self.host_scheduling.is_none() {
            return 0;
        }
        self.current_execution_payload_bytes()
            .saturating_add(self.displaced_execution_bytes.get())
            .saturating_add(
                self.pending_tail_call
                    .as_ref()
                    .map_or(0, |call| values_bytes(call)),
            )
    }

    /// Check at interpreter/native safepoints, including operations which
    /// grow primitive values without allocating a managed object. An
    /// unchanged payload avoids rescanning all Promise and task owners.
    pub(super) fn synchronize_execution_payload(&mut self) -> Result<(), RuntimeError> {
        self.check_scheduling_failure()?;
        if self.host_scheduling.is_none() {
            return Ok(());
        }
        let bytes = self.owned_execution_payload_bytes();
        if bytes != self.last_execution_payload_bytes {
            self.synchronize_scheduling_usage()?;
            self.last_execution_payload_bytes = bytes;
        }
        Ok(())
    }
}
