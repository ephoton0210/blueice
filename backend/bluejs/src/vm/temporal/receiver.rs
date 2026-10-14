// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Receiver brand checks for Temporal prototype members.
//!
//! Every Temporal prototype getter and method starts with
//! `RequireInternalSlot(this, [[InitializedTemporal<Type>]])`: a receiver that
//! is not exactly that type -- a primitive, an ordinary object (even one that
//! inherits from the prototype), the prototype itself, or a *different*
//! Temporal type -- throws `TypeError` before any argument is read.
//!
//! One `NativeFunction` can back the same-named member of more than one
//! prototype (`year` on `PlainDate`, `PlainDateTime`, `PlainYearMonth` and
//! `ZonedDateTime`; `add` on `PlainDate` and `PlainDateTime`), and those
//! natives dispatch on the receiver's own kind. The brand is therefore a
//! property of the *function*, not of the receiver: [`temporal_receiver_kind`]
//! is the single table of it, and `native_call` applies it once, up front,
//! for every Temporal native.
//!
//! [`temporal_receiver_kind`]: NativeFunction::temporal_receiver_kind

use super::*;

/// Owned receiver data captured before any observable argument access. The
/// native frame retains `value` through callbacks; the snapshot itself neither
/// dereferences an ObjectId nor registers an independent GC root.
pub(in super::super) struct ValidatedTemporalReceiver {
    value: Value,
    data: TemporalValue,
}

impl ValidatedTemporalReceiver {
    pub(in super::super) fn value(&self) -> &Value {
        &self.value
    }
    pub(in super::super) fn data(&self) -> &TemporalValue {
        &self.data
    }
    pub(in super::super) fn instant_epoch(&self) -> BigInt {
        self.data.epoch_nanoseconds.clone()
    }
    pub(in super::super) fn time_fields(&self) -> epoch::CivilTime {
        (
            self.data.hour,
            self.data.minute,
            self.data.second,
            self.data.millisecond,
            self.data.microsecond,
            self.data.nanosecond,
        )
    }
}

impl Vm {
    /// Validate the exact native receiver kind and capture its immutable slots.
    pub(in super::super) fn validate_temporal_receiver(
        &self,
        receiver: &Value,
        kind: TemporalKind,
    ) -> Result<ValidatedTemporalReceiver, RuntimeError> {
        let actual = match receiver.object_id() {
            Some(object) => self.heap.temporal_kind(object)?,
            None => None,
        };
        if actual != Some(kind) {
            return Err(RuntimeError::TypeError(format!(
                "receiver is not a Temporal.{}",
                kind.name()
            )));
        }
        // The immutable kind lookup validated this exact object. These reads
        // execute neither JavaScript nor a managed-heap collection.
        let data = self
            .heap
            .temporal_value(receiver.object_id().unwrap())
            .expect("the validated Temporal object remains live during immutable slot capture")
            .expect("the validated Temporal kind owns its value slot");
        Ok(ValidatedTemporalReceiver {
            value: receiver.clone(),
            data,
        })
    }
}
