// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native member kind table; queried independently of receiver data extraction.

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
