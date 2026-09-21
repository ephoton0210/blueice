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

impl NativeFunction {
    /// The Temporal type a prototype member's `this` must be, or `None` for a
    /// native that has no such receiver: constructors, the static
    /// `from`/`compare`/`fromEpoch*` functions, `Temporal.Now`'s members, and
    /// every `valueOf` (which throws `TypeError` for any receiver at all).
    pub(in super::super) fn temporal_receiver_kind(self) -> Option<TemporalKind> {
        Some(match self {
            Self::TemporalGetter(kind, _)
            | Self::TemporalWithCalendar(kind)
            | Self::TemporalPlainToZonedDateTime(kind)
            | Self::TemporalDateWith(kind)
            | Self::TemporalDateAdd(kind)
            | Self::TemporalDateSubtract(kind)
            | Self::TemporalDateUntil(kind)
            | Self::TemporalDateSince(kind)
            | Self::TemporalDateEquals(kind)
            | Self::TemporalDateToString(kind)
            | Self::TemporalDateToJson(kind)
            | Self::TemporalDateToLocaleString(kind) => kind,
            Self::TemporalInstantToZonedDateTimeIso
            | Self::TemporalInstantAdd
            | Self::TemporalInstantSubtract
            | Self::TemporalInstantRound
            | Self::TemporalInstantUntil
            | Self::TemporalInstantSince
            | Self::TemporalInstantEquals
            | Self::TemporalInstantToString
            | Self::TemporalInstantToLocaleString
            | Self::TemporalInstantToJson => TemporalKind::Instant,
            Self::TemporalPlainTimeAdd
            | Self::TemporalPlainTimeSubtract
            | Self::TemporalPlainTimeRound
            | Self::TemporalPlainTimeUntil
            | Self::TemporalPlainTimeSince
            | Self::TemporalPlainTimeEquals
            | Self::TemporalPlainTimeWith
            | Self::TemporalPlainTimeToString
            | Self::TemporalPlainTimeToJson
            | Self::TemporalPlainTimeToLocaleString => TemporalKind::PlainTime,
            Self::TemporalDurationWith
            | Self::TemporalDurationNegated
            | Self::TemporalDurationAbs
            | Self::TemporalDurationAdd
            | Self::TemporalDurationSubtract
            | Self::TemporalDurationRound
            | Self::TemporalDurationTotal
            | Self::TemporalDurationToString
            | Self::TemporalDurationToJson
            | Self::TemporalDurationToLocaleString => TemporalKind::Duration,
            Self::TemporalPlainDateToPlainDateTime
            | Self::TemporalPlainDateToPlainYearMonth
            | Self::TemporalPlainDateToPlainMonthDay => TemporalKind::PlainDate,
            Self::TemporalPlainDateTimeToPlainDate
            | Self::TemporalPlainDateTimeToPlainTime
            | Self::TemporalPlainDateTimeWithPlainTime
            | Self::TemporalPlainDateTimeRound => TemporalKind::PlainDateTime,
            Self::TemporalYearMonthWith
            | Self::TemporalYearMonthAdd
            | Self::TemporalYearMonthSubtract
            | Self::TemporalYearMonthUntil
            | Self::TemporalYearMonthSince
            | Self::TemporalYearMonthEquals
            | Self::TemporalYearMonthToString
            | Self::TemporalYearMonthToJson
            | Self::TemporalYearMonthToLocaleString
            | Self::TemporalYearMonthToPlainDate => TemporalKind::PlainYearMonth,
            Self::TemporalMonthDayWith
            | Self::TemporalMonthDayEquals
            | Self::TemporalMonthDayToString
            | Self::TemporalMonthDayToJson
            | Self::TemporalMonthDayToLocaleString
            | Self::TemporalMonthDayToPlainDate => TemporalKind::PlainMonthDay,
            Self::TemporalZonedDateTimeToLocaleString
            | Self::TemporalZonedDateTimeWith
            | Self::TemporalZonedDateTimeWithTimeZone
            | Self::TemporalZonedDateTimeWithPlainTime
            | Self::TemporalZonedDateTimeAdd
            | Self::TemporalZonedDateTimeSubtract
            | Self::TemporalZonedDateTimeRound
            | Self::TemporalZonedDateTimeUntil
            | Self::TemporalZonedDateTimeSince
            | Self::TemporalZonedDateTimeEquals
            | Self::TemporalZonedDateTimeToString
            | Self::TemporalZonedDateTimeToJson
            | Self::TemporalZonedDateTimeToInstant
            | Self::TemporalZonedDateTimeToPlainDate
            | Self::TemporalZonedDateTimeToPlainTime
            | Self::TemporalZonedDateTimeToPlainDateTime
            | Self::TemporalZonedDateTimeStartOfDay
            | Self::TemporalZonedDateTimeGetTimeZoneTransition => TemporalKind::ZonedDateTime,
            _ => return None,
        })
    }
}

impl Vm {
    /// `RequireInternalSlot(receiver, [[InitializedTemporal<kind>]])`: the
    /// receiver must be an object carrying a Temporal value of exactly `kind`.
    pub(in super::super) fn require_temporal_receiver(
        &self,
        receiver: &Value,
        kind: TemporalKind,
    ) -> Result<(), RuntimeError> {
        let actual = match receiver.object_id() {
            Some(object) => self.heap.temporal_kind(object)?,
            None => None,
        };
        if actual == Some(kind) {
            return Ok(());
        }
        Err(RuntimeError::TypeError(format!(
            "receiver is not a Temporal.{}",
            kind.name()
        )))
    }
}
