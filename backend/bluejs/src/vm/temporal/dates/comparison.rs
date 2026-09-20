// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `equals` and `compare` for `PlainDate` and `PlainDateTime`.

use super::super::*;

impl Vm {
    pub(in super::super::super) fn temporal_date_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        let other = self.temporal_to_matching(other_value, existing.kind, &Value::Undefined)?;
        let mut equal = existing.year == other.year
            && existing.month == other.month
            && existing.day == other.day
            && existing.calendar == other.calendar;
        if equal && existing.kind == TemporalKind::PlainDateTime {
            equal = existing.hour == other.hour
                && existing.minute == other.minute
                && existing.second == other.second
                && existing.millisecond == other.millisecond
                && existing.microsecond == other.microsecond
                && existing.nanosecond == other.nanosecond;
        }
        Ok(Value::Bool(equal))
    }

    pub(in super::super::super) fn temporal_date_compare(
        &mut self,
        kind: TemporalKind,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let a = self.temporal_to_matching(one, kind, &Value::Undefined)?;
        let b = self.temporal_to_matching(two, kind, &Value::Undefined)?;
        let ord = (
            a.year,
            a.month,
            a.day,
            a.hour,
            a.minute,
            a.second,
            a.millisecond,
            a.microsecond,
            a.nanosecond,
        )
            .cmp(&(
                b.year,
                b.month,
                b.day,
                b.hour,
                b.minute,
                b.second,
                b.millisecond,
                b.microsecond,
                b.nanosecond,
            ));
        Ok(Value::Number(match ord {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }
}
