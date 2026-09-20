// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    /// The calendar-agnostic gate shared by `add`/`subtract`/`round`/`total`/
    /// `compare`. Where the specification requires a `relativeTo` this engine
    /// cannot honour, the answer is a `RangeError`, never an approximation.
    pub(in super::super) fn temporal_duration_require_no_calendar_units(
        record: &blueice_ecma402::DurationRecord,
        units: &[rounding::TemporalUnit],
    ) -> Result<(), RuntimeError> {
        if Self::temporal_duration_largest_unit(record).is_calendar()
            || units.iter().any(|unit| unit.is_calendar())
        {
            return Err(RuntimeError::RangeError(
                "a Temporal.Duration with years, months or weeks needs a relativeTo anchor".into(),
            ));
        }
        Ok(())
    }

    pub(in super::super) fn temporal_duration_with(
        &mut self,
        receiver: &Value,
        like: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        if !matches!(like, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration.prototype.with requires a Duration-like object".into(),
            ));
        }
        let mut fields = [
            record.years,
            record.months,
            record.weeks,
            record.days,
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        ];
        let mut present = false;
        for (name, index) in DURATION_FIELDS_IN_READ_ORDER {
            let value = self.get_property(like, &name.into())?;
            if value == Value::Undefined {
                continue;
            }
            present = true;
            fields[index] = self.temporal_duration_integer(&value, name)?;
        }
        if !present {
            return Err(RuntimeError::TypeError(
                "Temporal.Duration.prototype.with requires at least one duration field".into(),
            ));
        }
        self.temporal_duration_create(fields)
    }

    pub(in super::super) fn temporal_duration_negated(
        &mut self,
        receiver: &Value,
        absolute: bool,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let map = |value: i128| if absolute { value.abs() } else { -value };
        // Negating or taking the magnitude of every field at once preserves
        // both the common-sign and the range invariants, so this cannot fail.
        self.alloc_temporal_value(
            Self::temporal_duration_value(blueice_ecma402::DurationRecord {
                years: map(record.years),
                months: map(record.months),
                weeks: map(record.weeks),
                days: map(record.days),
                hours: map(record.hours),
                minutes: map(record.minutes),
                seconds: map(record.seconds),
                milliseconds: map(record.milliseconds),
                microseconds: map(record.microseconds),
                nanoseconds: map(record.nanoseconds),
            }),
            false,
        )
    }

    pub(in super::super) fn temporal_duration_add(
        &mut self,
        receiver: &Value,
        other: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_duration_receiver(receiver)?;
        let mut two = self.temporal_duration_from_value(other)?;
        if negate {
            two = blueice_ecma402::DurationRecord {
                years: -two.years,
                months: -two.months,
                weeks: -two.weeks,
                days: -two.days,
                hours: -two.hours,
                minutes: -two.minutes,
                seconds: -two.seconds,
                milliseconds: -two.milliseconds,
                microseconds: -two.microseconds,
                nanoseconds: -two.nanoseconds,
            };
        }
        // `AddDurations` balances the sum up to the larger of the two
        // operands' own largest units — a calendar one has no fixed length,
        // so it is rejected outright rather than balanced.
        let largest = Self::temporal_duration_largest_unit(&one)
            .max(Self::temporal_duration_largest_unit(&two));
        Self::temporal_duration_require_no_calendar_units(&one, &[largest])?;
        Self::temporal_duration_require_no_calendar_units(&two, &[])?;
        let total = duration_math::TimeDuration::from_record_with_24_hour_days(&one)
            .total_nanoseconds()
            + duration_math::TimeDuration::from_record_with_24_hour_days(&two).total_nanoseconds();
        let balanced =
            duration_math::TimeDuration::from_nanoseconds(total).balance_with_days(largest);
        self.temporal_duration_create([
            0,
            0,
            0,
            balanced[0],
            balanced[1],
            balanced[2],
            balanced[3],
            balanced[4],
            balanced[5],
            balanced[6],
        ])
    }

    pub(in super::super) fn temporal_duration_round(
        &mut self,
        receiver: &Value,
        round_to: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let (shorthand, options) = self.temporal_duration_round_to(round_to, "round")?;
            // The specification reads every option, in alphabetical order,
            // before any algorithmic validation happens.
            let (requested_largest, anchor, increment, mode, requested_smallest) = match &shorthand
            {
                Some(text) => (
                    UnitOption::Unset,
                    None,
                    1,
                    blueice_ecma402::NumberRoundingMode::HalfExpand,
                    UnitOption::Unit(Self::temporal_duration_unit_name(text, "smallestUnit")?),
                ),
                None => {
                    let largest =
                        self.temporal_duration_unit_option(&options, "largestUnit", true)?;
                    let relative_to = self.get_property(&options, &"relativeTo".into())?;
                    let anchor = self.temporal_duration_relative_to(&relative_to)?;
                    let increment = self.temporal_rounding_increment(&options)?;
                    let mode = self.temporal_rounding_mode(
                        &options,
                        blueice_ecma402::NumberRoundingMode::HalfExpand,
                    )?;
                    let smallest =
                        self.temporal_duration_unit_option(&options, "smallestUnit", false)?;
                    (largest, anchor, increment, mode, smallest)
                }
            };
            if requested_largest == UnitOption::Unset && requested_smallest == UnitOption::Unset {
                return Err(RuntimeError::RangeError(
                    "Temporal.Duration.prototype.round requires largestUnit or smallestUnit".into(),
                ));
            }
            let smallest = requested_smallest
                .unit()
                .unwrap_or(rounding::TemporalUnit::Nanosecond);
            // A smallestUnit larger than the duration's own largest unit
            // raises the default largestUnit with it, so e.g. rounding
            // 86,399 seconds to days yields one day rather than zero.
            let default_largest = Self::temporal_duration_largest_unit(&record).max(smallest);
            let largest = requested_largest.unit().unwrap_or(default_largest);
            if smallest > largest {
                return Err(RuntimeError::RangeError(
                    "smallestUnit must not be larger than largestUnit".into(),
                ));
            }
            if let Some(maximum) = smallest.maximum_rounding_increment() {
                if increment >= maximum || maximum % increment != 0 {
                    return Err(RuntimeError::RangeError(
                        "roundingIncrement does not divide evenly into the next larger unit".into(),
                    ));
                }
            }
            if increment > 1 && smallest != largest && smallest >= rounding::TemporalUnit::Day {
                return Err(RuntimeError::RangeError(
                    "a date-unit roundingIncrement above 1 cannot also balance to a larger unit"
                        .into(),
                ));
            }
            // A `Zoned` anchor is a different computation from a `Plain` one
            // even for a blank duration or a purely time-granularity round: a
            // day is not a fixed 24 hours there, so the receiver's own real day
            // length decides whether a rounded remainder carries into `days`
            // (`case-where-relativeto-affects-rounding-mode-half-even.js`,
            // `next-day-out-of-range.js`). The specification has `round` add the
            // whole duration to the anchor and then take
            // `DifferenceZonedDateTimeWithRounding` between the two -- exactly
            // what `ZonedDateTime.prototype.until` does.
            if let Some(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns,
                local_date,
                local_time,
            }) = &anchor
            {
                let target = Self::temporal_duration_zoned_target(
                    zone,
                    *calendar,
                    epoch_ns,
                    *local_date,
                    *local_time,
                    &record,
                )?;
                let origin = zoned_difference::ZonedOrigin {
                    zone,
                    calendar: *calendar,
                    epoch_nanoseconds: epoch_ns,
                    date: *local_date,
                    time: *local_time,
                };
                let internal = zoned_difference::difference_with_rounding(
                    &origin, &target, largest, increment, smallest, mode,
                )
                .ok_or_else(|| {
                    RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                })?;
                return self
                    .temporal_duration_create(internal.into_fields(largest).map(i128::from));
            }
            // A blank duration rounds to a blank duration in every unit: zero
            // is an exact multiple of any increment, and balancing zero
            // yields zero. Given an anchor, that is the whole answer even for
            // a calendar unit, with no calendar arithmetic involved.
            if anchor.is_some() && record.sign() == 0 {
                return self.temporal_duration_create([0; 10]);
            }
            if let Some(anchor) = &anchor {
                Self::temporal_duration_anchor_datetime_in_range(anchor.date())?;
            }
            let needs_calendar = Self::temporal_duration_largest_unit(&record).is_calendar()
                || largest.is_calendar()
                || smallest.is_calendar();
            if needs_calendar {
                let anchor = anchor.ok_or_else(|| {
                    RuntimeError::RangeError(
                        "a Temporal.Duration with years, months or weeks needs a relativeTo \
                         anchor"
                            .into(),
                    )
                })?;
                return self.temporal_duration_round_relative(
                    anchor.calendar(),
                    anchor.date(),
                    &record,
                    largest,
                    smallest,
                    increment,
                    mode,
                );
            }
            let step = smallest
                .nanoseconds()
                .expect("a non-calendar smallestUnit always has an exact length")
                * increment;
            let balanced = duration_math::TimeDuration::from_record_with_24_hour_days(&record)
                .rounded_to_step(step, mode)
                .balance_with_days(largest);
            self.temporal_duration_create([
                0,
                0,
                0,
                balanced[0],
                balanced[1],
                balanced[2],
                balanced[3],
                balanced[4],
                balanced[5],
                balanced[6],
            ])
        })();
        self.stack.truncate(base);
        result
    }

    /// The calendar-aware half of `round`: `smallestUnit`/`largestUnit`
    /// involves a `year`/`month`/`week`, or the receiver's own largest
    /// nonzero field does, so the answer needs `anchor + record`'s real
    /// calendar-date landing rather than a fixed-length nanosecond total.
    /// Mirrors `plain_date::round_calendar_duration`'s own algorithm shape
    /// (`temporal_date_difference` uses the identical split, between two
    /// already-known dates instead of an anchor plus a duration to add).
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn temporal_duration_round_relative(
        &mut self,
        calendar: AnyCalendarKind,
        anchor: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
        largest: rounding::TemporalUnit,
        smallest: rounding::TemporalUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Result<Value, RuntimeError> {
        const DAY_NS: i128 = 86_400_000_000_000;
        let time_total = duration_math::TimeDuration::from_fields(
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        )
        .total_nanoseconds();
        if smallest >= rounding::TemporalUnit::Day {
            // Deliberately *not* `plain_date::round_calendar_duration` here:
            // that function only ever sees a whole-day remainder (it takes
            // two already-known dates), so a duration whose only remaining
            // content below `largestUnit` is a sub-day time part would lose
            // exactly the precision `ceil`/`floor`/`halfEven`/etc. need to
            // decide whether that remainder rounds up — Test262's
            // `roundingmode-ceil.js` is what catches this (a `largestUnit:
            // "years"`/no explicit `smallestUnit` case whose only remaining
            // content is ~40.5 leftover hours must still round the day count
            // up under "ceil"). This reimplements the same bracketing shape
            // `round_calendar_duration`/its private `round_month_or_year`
            // use, but carries the exact nanosecond remainder all the way
            // through the rounding decision instead of pre-folding it into a
            // possibly-truncated day count.
            return self.temporal_duration_round_calendar_exact(
                calendar, anchor, record, time_total, largest, smallest, increment, mode,
            );
        }
        let (intermediate, ns_of_day) =
            Self::temporal_duration_intermediate(calendar, anchor, record)?;
        // `smallest` is a time unit: round the exact sub-day remainder first,
        // then recombine with the whole-day calendar difference — the same
        // shape `temporal_date_difference`'s own sub-day branch uses, with
        // `anchor`/`intermediate` standing in for that function's `from`/
        // `adjusted_to` and `ns_of_day` standing in for its `time_diff` (both
        // already exact and sign-consistent, so no day-adjustment step is
        // needed here the way two independent wall-clock endpoints require).
        let time_unit = match smallest {
            rounding::TemporalUnit::Hour => rounding::TimeUnit::Hour,
            rounding::TemporalUnit::Minute => rounding::TimeUnit::Minute,
            rounding::TemporalUnit::Second => rounding::TimeUnit::Second,
            rounding::TemporalUnit::Millisecond => rounding::TimeUnit::Millisecond,
            rounding::TemporalUnit::Microsecond => rounding::TimeUnit::Microsecond,
            _ => rounding::TimeUnit::Nanosecond,
        };
        let rounded = duration_math::TimeDuration::from_nanoseconds(ns_of_day)
            .round(time_unit, increment, mode);
        let total = rounded.total_nanoseconds();
        let day_carry = total / DAY_NS;
        let ns_of_day = total % DAY_NS;
        let time_largest = if largest >= rounding::TemporalUnit::Day {
            rounding::TimeUnit::Hour
        } else {
            match largest {
                rounding::TemporalUnit::Hour => rounding::TimeUnit::Hour,
                rounding::TemporalUnit::Minute => rounding::TimeUnit::Minute,
                rounding::TemporalUnit::Second => rounding::TimeUnit::Second,
                rounding::TemporalUnit::Millisecond => rounding::TimeUnit::Millisecond,
                rounding::TemporalUnit::Microsecond => rounding::TimeUnit::Microsecond,
                _ => rounding::TimeUnit::Nanosecond,
            }
        };
        let balanced =
            duration_math::TimeDuration::from_nanoseconds(ns_of_day).balance_to(time_largest);
        let whole_days = plain_date::calendar_difference_date(
            calendar,
            anchor,
            intermediate,
            plain_date::DateUnit::Day,
        )
        .3;
        let total_days = whole_days + day_carry as i64;
        let day_target =
            plain_date::calendar_add_date(calendar, anchor, 0, 0, 0, total_days, false)
                .ok_or_else(|| {
                    RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                })?;
        let (years, months, weeks, days) = plain_date::calendar_difference_date(
            calendar,
            anchor,
            day_target,
            Self::temporal_unit_to_date_unit(largest),
        );
        self.temporal_duration_create([
            years as i128,
            months as i128,
            weeks as i128,
            days as i128,
            i128::from(balanced[0]),
            i128::from(balanced[1]),
            i128::from(balanced[2]),
            i128::from(balanced[3]),
            i128::from(balanced[4]),
            i128::from(balanced[5]),
        ])
    }

    /// `round`'s calendar-aware `day`/`week`/`month`/`year`-granularity
    /// rounding, keeping the exact sub-day nanosecond remainder alive all
    /// the way through the rounding decision (see the caller's own comment
    /// for why `plain_date::round_calendar_duration` can't be reused
    /// directly here).
    ///
    /// Also fixes a real, separate discrepancy found while deriving this
    /// against `roundingmode-ceil.js`'s own `weeks` case:
    /// `round_calendar_duration`'s `Week` branch only places its rounded
    /// value in the `weeks` output field when `largestUnit` is itself
    /// `"weeks"`, folding it into `days` (as an always-multiple-of-7 value)
    /// otherwise — but Temporal's actual field-population rule is that
    /// `weeks` appears whenever `smallestUnit` is `"weeks"`, regardless of
    /// `largestUnit` (`{ largestUnit: "years", smallestUnit: "weeks" }` on a
    /// multi-year duration still reports a real `weeks` field alongside
    /// `years`/`months`, never a `days` value in the hundreds). This
    /// function's own `smallest == Week` handling corrects that locally
    /// rather than by editing the shared, already-merged
    /// `plain_date::round_calendar_duration` (out of this pass's file
    /// scope — see this phase's own scope notes); `temporal_date_difference`
    /// (`PlainDate`/`PlainDateTime.prototype.since`/`until`) calls the
    /// unmodified original directly and likely has the identical gap for
    /// the same option combination, which is worth its own owner's
    /// attention.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn temporal_duration_round_calendar_exact(
        &mut self,
        calendar: AnyCalendarKind,
        anchor: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
        time_total: i128,
        largest: rounding::TemporalUnit,
        smallest: rounding::TemporalUnit,
        increment: i128,
        mode: blueice_ecma402::NumberRoundingMode,
    ) -> Result<Value, RuntimeError> {
        const DAY_NS: i128 = 86_400_000_000_000;
        let date_unit_largest = Self::temporal_unit_to_date_unit(largest);
        // The date-only landing point (no time contribution at all yet) —
        // used both to find the exact pre-rounding remainder below
        // `largestUnit` and, for `month`/`year`, as the fractional-position
        // anchor `round_month_or_year` itself would use.
        let date_only = plain_date::calendar_add_date(
            calendar,
            anchor,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            record.days as i64,
            false,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        // See `temporal_duration_intermediate`'s own identical comment:
        // `calendar_add_date` alone doesn't catch a numerically valid but
        // unrepresentable landing date. Checked against the *whole* date part
        // including the time component folded into whole days (not just
        // `date_only`, which omits it), since a huge time component alone
        // (e.g. `Number.MAX_SAFE_INTEGER` seconds, `record.days == 0`) is
        // exactly what
        // `relativeto-plaindate-large-time-component-out-of-range.js` checks
        // for every `smallestUnit` (`year`/`month`/`week`), not only the
        // `Day`/`Week` branch below.
        let time_folded_days = record.days + time_total / DAY_NS;
        let date_with_time = plain_date::calendar_add_date(
            calendar,
            anchor,
            record.years as i64,
            record.months as i64,
            record.weeks as i64,
            time_folded_days as i64,
            false,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
        })?;
        if !epoch::is_date_within_limits(date_only) || !epoch::is_date_within_limits(date_with_time)
        {
            return Err(RuntimeError::RangeError(
                "Temporal date arithmetic is out of range".into(),
            ));
        }

        if matches!(
            smallest,
            rounding::TemporalUnit::Day | rounding::TemporalUnit::Week
        ) {
            let (years0, months0, weeks0, days0) = plain_date::calendar_difference_date(
                calendar,
                anchor,
                date_only,
                date_unit_largest,
            );
            let remainder_ns = i128::from(weeks0 * 7 + days0) * DAY_NS + time_total;
            let step_days: i128 = if smallest == rounding::TemporalUnit::Week {
                7
            } else {
                1
            };
            let step_ns = DAY_NS * step_days * increment;
            let rounded_ns = rounding::round_to_increment(remainder_ns, step_ns, mode);
            let rounded_days = (rounded_ns / DAY_NS) as i64;
            // `calendar_difference_date` only ever splits out a years/months
            // component when its own `largest_unit` is `Year`/`Month`; for a
            // `Day`/`Week` `largestUnit` there is no such split (the whole
            // duration collapses to a flat day count from `anchor`), so the
            // pre-offset must match that or the final re-split below would
            // double-count a years/months contribution. Crucially, the
            // pre-offset uses `years0`/`months0` — the *calendar-bracketed*
            // decomposition of the whole `date_only` landing point computed
            // just above — rather than `record.years`/`record.months`
            // directly: the record's own `weeks`/`days` (and any leftover
            // time) can themselves push the whole-months/-years count past
            // what the record's own `years`/`months` fields alone would
            // suggest (e.g. 6 months + 7 weeks + 8 days lands on a real
            // 7th month), and it is *that* landing which must anchor the
            // rounding step, not the record's raw field split.
            let years_months_point = if matches!(
                date_unit_largest,
                plain_date::DateUnit::Year | plain_date::DateUnit::Month
            ) {
                plain_date::calendar_add_date(calendar, anchor, years0, months0, 0, 0, false)
                    .ok_or_else(|| {
                        RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                    })?
            } else {
                anchor
            };
            let day_target = plain_date::calendar_add_date(
                calendar,
                years_months_point,
                0,
                0,
                0,
                rounded_days,
                false,
            )
            .ok_or_else(|| {
                RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
            })?;
            let (years, months, weeks, days) = plain_date::calendar_difference_date(
                calendar,
                anchor,
                day_target,
                date_unit_largest,
            );
            // See this function's own doc comment: `weeks` must carry the
            // rounded value whenever `smallest` is `week`, even if
            // `largestUnit` folded it into `days` above.
            let (weeks, days) = if smallest == rounding::TemporalUnit::Week
                && largest != rounding::TemporalUnit::Week
            {
                (days / 7, 0)
            } else {
                (weeks, days)
            };
            return self.temporal_duration_create([
                years as i128,
                months as i128,
                weeks as i128,
                days as i128,
                0,
                0,
                0,
                0,
                0,
                0,
            ]);
        }

        // `smallest` is `month` or `year`: the anchor-relative fractional
        // bracketing every Temporal implementation uses (mirrors
        // `plain_date::round_month_or_year`'s own numerator/denominator
        // exact-integer shape), generalized to weigh the exact leftover
        // nanoseconds rather than only a whole-day position.
        let sign = match plain_date::compare_iso_date(anchor, date_only) {
            std::cmp::Ordering::Less => 1_i64,
            std::cmp::Ordering::Greater => -1,
            std::cmp::Ordering::Equal if time_total == 0 => {
                return self.temporal_duration_create([0; 10]);
            }
            std::cmp::Ordering::Equal => {
                if time_total < 0 {
                    -1
                } else {
                    1
                }
            }
        };
        // Decompose at `smallest`'s own granularity (not `largestUnit`'s) to
        // get the true combined count: `calendar_difference_date(...,
        // largest_unit: Year)` only ever returns a *remainder* months field
        // (0..11), never years-and-months combined, whereas `smallest ==
        // "months"` needs the single combined total (mirrors
        // `round_calendar_duration`'s own Month branch computing
        // `total_months` this same way when `largestUnit` is `"years"`).
        let count_unit = Self::temporal_unit_to_date_unit(smallest);
        let (count_years, count_months, _, _) =
            plain_date::calendar_difference_date(calendar, anchor, date_only, count_unit);
        // `calendar_difference_date`'s own `Year`/`Month` branches put the
        // combined count in different tuple slots (`years` when its own
        // `largest_unit` is `Year`, `months` — already years*12+months
        // combined — when it is `Month`).
        let count = if smallest == rounding::TemporalUnit::Year {
            count_years
        } else {
            count_months
        };
        let add_n = |n: i64| -> epoch::CivilDate {
            let (y, m) = if smallest == rounding::TemporalUnit::Year {
                (n, 0)
            } else {
                (0, n)
            };
            plain_date::calendar_add_date(calendar, anchor, y, m, 0, 0, false)
                .expect("constrain-mode single-unit addition always succeeds")
        };
        let lower = add_n(count);
        let upper = add_n(count + sign);
        let total_span_ns = i128::from(
            (plain_date::iso_date_to_epoch_days(upper) - plain_date::iso_date_to_epoch_days(lower))
                .unsigned_abs(),
        ) * DAY_NS;
        let progressed_ns = i128::from(
            (plain_date::iso_date_to_epoch_days(date_only)
                - plain_date::iso_date_to_epoch_days(lower))
            .unsigned_abs(),
        ) * DAY_NS
            + time_total.unsigned_abs() as i128;

        let magnitude = i128::from(count.unsigned_abs());
        let increment_i128 = increment.max(1);
        let lower_multiple = (magnitude / increment_i128) * increment_i128;
        let upper_multiple = lower_multiple + increment_i128;
        let extra = magnitude - lower_multiple;
        let numerator = extra * total_span_ns + progressed_ns;
        let denominator = increment_i128 * total_span_ns;
        let round_up = if denominator == 0 || numerator == 0 {
            false
        } else {
            use blueice_ecma402::NumberRoundingMode as Mode;
            match mode {
                Mode::Ceil => sign > 0,
                Mode::Floor => sign < 0,
                Mode::Expand => true,
                Mode::Trunc => false,
                Mode::HalfCeil => {
                    if sign > 0 {
                        2 * numerator >= denominator
                    } else {
                        2 * numerator > denominator
                    }
                }
                Mode::HalfFloor => {
                    if sign < 0 {
                        2 * numerator >= denominator
                    } else {
                        2 * numerator > denominator
                    }
                }
                Mode::HalfExpand => 2 * numerator >= denominator,
                Mode::HalfTrunc => 2 * numerator > denominator,
                Mode::HalfEven => {
                    if 2 * numerator == denominator {
                        (lower_multiple / increment_i128) % 2 != 0
                    } else {
                        2 * numerator > denominator
                    }
                }
            }
        };
        let final_magnitude = if round_up {
            upper_multiple
        } else {
            lower_multiple
        };
        let rounded_count = sign * (final_magnitude as i64);
        let (years, months) = if smallest == rounding::TemporalUnit::Year {
            (rounded_count, 0)
        } else if largest == rounding::TemporalUnit::Year {
            (rounded_count / 12, rounded_count % 12)
        } else {
            (0, rounded_count)
        };
        self.temporal_duration_create([years as i128, months as i128, 0, 0, 0, 0, 0, 0, 0, 0])
    }

    pub(in super::super) fn temporal_duration_total(
        &mut self,
        receiver: &Value,
        total_of: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let (shorthand, options) = self.temporal_duration_round_to(total_of, "total")?;
            let (unit, anchor) = match &shorthand {
                Some(text) => (Self::temporal_duration_unit_name(text, "unit")?, None),
                None => {
                    let relative_to = self.get_property(&options, &"relativeTo".into())?;
                    let anchor = self.temporal_duration_relative_to(&relative_to)?;
                    let unit = self
                        .temporal_duration_unit_option(&options, "unit", false)?
                        .unit()
                        .ok_or_else(|| {
                            RuntimeError::RangeError(
                                "Temporal.Duration.prototype.total requires unit".into(),
                            )
                        })?;
                    (unit, anchor)
                }
            };
            // See `round` above for why a `Zoned` anchor is dispatched before
            // even the blank-duration shortcut: resolving its real day
            // length/bracket can itself throw. `total` is the same target
            // instant followed by `DifferenceZonedDateTimeWithTotal`.
            if let Some(DurationAnchor::Zoned {
                calendar,
                zone,
                epoch_ns,
                local_date,
                local_time,
            }) = &anchor
            {
                let target = Self::temporal_duration_zoned_target(
                    zone,
                    *calendar,
                    epoch_ns,
                    *local_date,
                    *local_time,
                    &record,
                )?;
                let origin = zoned_difference::ZonedOrigin {
                    zone,
                    calendar: *calendar,
                    epoch_nanoseconds: epoch_ns,
                    date: *local_date,
                    time: *local_time,
                };
                let (numerator, denominator) = zoned_difference::difference_with_total(
                    &origin, &target, unit,
                )
                .ok_or_else(|| {
                    RuntimeError::RangeError("Temporal date arithmetic is out of range".into())
                })?;
                return Ok(Value::Number(rounding::exact_ratio_to_f64(
                    numerator,
                    denominator,
                )));
            }
            // A blank duration totals zero in every unit; see `round` above.
            if anchor.is_some() && record.sign() == 0 {
                return Ok(Value::Number(0.0));
            }
            if let Some(anchor) = &anchor {
                Self::temporal_duration_anchor_datetime_in_range(anchor.date())?;
            }
            let needs_calendar =
                Self::temporal_duration_largest_unit(&record).is_calendar() || unit.is_calendar();
            if needs_calendar {
                let anchor = anchor.ok_or_else(|| {
                    RuntimeError::RangeError(
                        "a Temporal.Duration with years, months or weeks needs a relativeTo \
                         anchor"
                            .into(),
                    )
                })?;
                return Ok(Value::Number(self.temporal_duration_total_relative(
                    anchor.calendar(),
                    anchor.date(),
                    &record,
                    unit,
                )?));
            }
            Ok(Value::Number(
                duration_math::TimeDuration::from_record_with_24_hour_days(&record).total_in(unit),
            ))
        })();
        self.stack.truncate(base);
        result
    }

    /// The calendar-aware half of `total`: the exact (fractional) count of
    /// `unit`s from `anchor` to `anchor + record`. For `day`/`week` this is
    /// exact days converted directly (with the `years`/`months`/`weeks` part
    /// resolved through the calendar first); for `month`/`year` it is the
    /// anchor-relative bracketing position `plain_date::round_month_or_year`
    /// also uses, computed here as a continuous fraction instead of rounded
    /// to an increment (that function is `round_calendar_duration`'s own
    /// private helper, so this mirrors its shape locally with the
    /// `pub(crate)` primitives `calendar_add_date`/`calendar_difference_date`
    /// rather than reaching into `plain_date.rs`, which Phase 26's own Track
    /// B scope leaves to its other in-flight owners).
    pub(in super::super) fn temporal_duration_total_relative(
        &mut self,
        calendar: AnyCalendarKind,
        anchor: epoch::CivilDate,
        record: &blueice_ecma402::DurationRecord,
        unit: rounding::TemporalUnit,
    ) -> Result<f64, RuntimeError> {
        const DAY_NS: i128 = 86_400_000_000_000;
        let (intermediate, ns_of_day) =
            Self::temporal_duration_intermediate(calendar, anchor, record)?;
        let day_fraction_abs = ns_of_day.unsigned_abs() as f64 / DAY_NS as f64;
        let sign = match plain_date::compare_iso_date(anchor, intermediate) {
            std::cmp::Ordering::Less => 1_i64,
            std::cmp::Ordering::Greater => -1,
            std::cmp::Ordering::Equal if ns_of_day == 0 => return Ok(0.0),
            // A same-day, sub-day-only remainder: its own sign (not the
            // date's, which didn't move) is the direction of travel.
            std::cmp::Ordering::Equal => {
                if ns_of_day < 0 {
                    -1
                } else {
                    1
                }
            }
        };
        match unit {
            // Both `day` and `week` are calendar-invariant fixed lengths
            // (7 days is 7 days regardless of calendar), so the total is
            // just the exact whole-day span from `anchor` plus the exact
            // sub-day remainder — computed as one integer ratio (never an
            // intermediate float) so it matches the spec's single
            // correctly-rounded final division exactly, bit for bit
            // (`relativeto-total-of-each-unit.js` is what catches a
            // two-step float version drifting by one ULP).
            rounding::TemporalUnit::Day | rounding::TemporalUnit::Week => {
                let total_ns = i128::from(
                    plain_date::iso_date_to_epoch_days(intermediate)
                        - plain_date::iso_date_to_epoch_days(anchor),
                ) * DAY_NS
                    + ns_of_day;
                let denominator = if unit == rounding::TemporalUnit::Week {
                    7 * DAY_NS
                } else {
                    DAY_NS
                };
                Ok(rounding::exact_ratio_to_f64(total_ns, denominator))
            }
            rounding::TemporalUnit::Month | rounding::TemporalUnit::Year => {
                let date_unit = Self::temporal_unit_to_date_unit(unit);
                let (years, months, _, _) =
                    plain_date::calendar_difference_date(calendar, anchor, intermediate, date_unit);
                let count = if unit == rounding::TemporalUnit::Year {
                    years
                } else {
                    months
                };
                let add_n = |n: i64| -> epoch::CivilDate {
                    let (y, m) = if unit == rounding::TemporalUnit::Year {
                        (n, 0)
                    } else {
                        (0, n)
                    };
                    plain_date::calendar_add_date(calendar, anchor, y, m, 0, 0, false)
                        .expect("constrain-mode single-unit addition always succeeds")
                };
                let lower = add_n(count);
                let upper = add_n(count + sign);
                // `add_n`'s own `calendar_add_date` call only range-checks
                // calendar-day validity (an i32-year check), not Temporal's
                // narrower representable range: bracketing one unit *past*
                // an anchor already at the exact max/min boundary lands on a
                // numerically valid but unrepresentable date without
                // otherwise erroring —
                // `throws-if-date-time-invalid-with-plaindate-relative.js`.
                if !epoch::is_date_within_limits(lower) || !epoch::is_date_within_limits(upper) {
                    return Err(RuntimeError::RangeError(
                        "Temporal date arithmetic is out of range".into(),
                    ));
                }
                let total_span = (plain_date::iso_date_to_epoch_days(upper)
                    - plain_date::iso_date_to_epoch_days(lower))
                .unsigned_abs() as f64;
                let progressed = (plain_date::iso_date_to_epoch_days(intermediate)
                    - plain_date::iso_date_to_epoch_days(lower))
                .unsigned_abs() as f64
                    + day_fraction_abs;
                let fraction = if total_span == 0.0 {
                    0.0
                } else {
                    progressed / total_span
                };
                Ok(count as f64 + (sign as f64) * fraction)
            }
            // A time-granularity `unit` still reaches this function whenever
            // the *record itself* has a nonzero year/month/week field (the
            // caller's `needs_calendar` gate is keyed on the record, not
            // `unit` alone) — e.g. `duration.total({ unit: "hours",
            // relativeTo })` on a multi-year `Duration`. The exact total
            // relative to `anchor` is just the whole-day span plus the exact
            // sub-day remainder, in `unit`s.
            _ => {
                let total_ns = i128::from(
                    plain_date::iso_date_to_epoch_days(intermediate)
                        - plain_date::iso_date_to_epoch_days(anchor),
                ) * DAY_NS
                    + ns_of_day;
                Ok(rounding::exact_ratio_to_f64(
                    total_ns,
                    unit.nanoseconds()
                        .expect("every time unit has an exact length"),
                ))
            }
        }
    }

    pub(in super::super) fn temporal_duration_compare(
        &mut self,
        one: &Value,
        two: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let one = self.temporal_duration_from_value(one)?;
        let two = self.temporal_duration_from_value(two)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_duration_options(options)?;
            let relative_to = self.get_property(&options, &"relativeTo".into())?;
            let anchor = self.temporal_duration_relative_to(&relative_to)?;
            // Field-identical durations compare equal before any unit is
            // considered, so even a calendar-unit duration compares to itself.
            if one == two {
                return Ok(Value::Number(0.0));
            }
            // A `Zoned` anchor, once either operand has a date part (years,
            // months, weeks or days -- `duration1.date != {}`): a day is not a
            // fixed 24 hours there (`twenty-five-hour-day.js`), so
            // `AddZonedDateTime` each operand's *full* duration to the same
            // anchor and compare the two resulting exact instants. Two purely
            // time-based durations never need the zone at all, so -- unlike the
            // date-part case -- they must not even range-check the anchor's
            // target instant (`relativeto-string-limits.js`: 5 minutes relative
            // to the last representable instant compares fine).
            let has_date_part = |record: &blueice_ecma402::DurationRecord| {
                record.years != 0 || record.months != 0 || record.weeks != 0 || record.days != 0
            };
            if let (
                Some(DurationAnchor::Zoned {
                    calendar,
                    zone,
                    epoch_ns,
                    local_date,
                    local_time,
                }),
                true,
            ) = (&anchor, has_date_part(&one) || has_date_part(&two))
            {
                let one_target = Self::temporal_duration_zoned_target(
                    zone,
                    *calendar,
                    epoch_ns,
                    *local_date,
                    *local_time,
                    &one,
                )?;
                let two_target = Self::temporal_duration_zoned_target(
                    zone,
                    *calendar,
                    epoch_ns,
                    *local_date,
                    *local_time,
                    &two,
                )?;
                return Ok(Value::Number(match one_target.cmp(&two_target) {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                }));
            }
            let needs_calendar = Self::temporal_duration_largest_unit(&one).is_calendar()
                || Self::temporal_duration_largest_unit(&two).is_calendar();
            if needs_calendar {
                let anchor = anchor.ok_or_else(|| {
                    RuntimeError::RangeError(
                        "a Temporal.Duration with years, months or weeks needs a relativeTo \
                         anchor"
                            .into(),
                    )
                })?;
                // The anchor is only converted to a date-time (and so judged
                // against its tighter range) once calendar arithmetic needs it:
                // `DateDurationDays` returns a plain day count without touching
                // the anchor when there are no years, months or weeks
                // (`relativeto-string-limits.js`).
                Self::temporal_duration_anchor_datetime_in_range(anchor.date())?;
                let calendar = anchor.calendar();
                let anchor_date = anchor.date();
                // Both operands land relative to the *same* anchor, so their
                // exact `(whole days, sub-day nanoseconds)` pairs compare
                // lexicographically exactly as their true nanosecond totals
                // would (each remainder's magnitude stays under one day).
                let (one_date, one_ns) =
                    Self::temporal_duration_intermediate(calendar, anchor_date, &one)?;
                let (two_date, two_ns) =
                    Self::temporal_duration_intermediate(calendar, anchor_date, &two)?;
                let one_days = plain_date::iso_date_to_epoch_days(one_date)
                    - plain_date::iso_date_to_epoch_days(anchor_date);
                let two_days = plain_date::iso_date_to_epoch_days(two_date)
                    - plain_date::iso_date_to_epoch_days(anchor_date);
                return Ok(Value::Number(
                    match (one_days, one_ns).cmp(&(two_days, two_ns)) {
                        std::cmp::Ordering::Less => -1.0,
                        std::cmp::Ordering::Equal => 0.0,
                        std::cmp::Ordering::Greater => 1.0,
                    },
                ));
            }
            let one = duration_math::TimeDuration::from_record_with_24_hour_days(&one)
                .total_nanoseconds();
            let two = duration_math::TimeDuration::from_record_with_24_hour_days(&two)
                .total_nanoseconds();
            Ok(Value::Number(match one.cmp(&two) {
                std::cmp::Ordering::Less => -1.0,
                std::cmp::Ordering::Equal => 0.0,
                std::cmp::Ordering::Greater => 1.0,
            }))
        })();
        self.stack.truncate(base);
        result
    }

    /// `GetTemporalFractionalSecondDigitsOption`: `auto` (the default, `None`
    /// here) or a digit count in `0..=9`. A non-Number value must stringify to
    /// exactly `"auto"`; a Number is floored rather than required to be
    /// integral.
    pub(in super::super) fn temporal_duration_fractional_digits(
        &mut self,
        options: &Value,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &"fractionalSecondDigits".into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        if !matches!(value, Value::Number(_)) {
            let text = self
                .coerce_string(&value)?
                .to_utf8()
                .map_err(|_| RuntimeError::RangeError("invalid fractionalSecondDigits".into()))?;
            if text == "auto" {
                return Ok(None);
            }
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits".into(),
            ));
        }
        let digits = self.coerce_number(&value)?;
        if !digits.is_finite() {
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits".into(),
            ));
        }
        let count = digits.floor();
        if !(0.0..=9.0).contains(&count) {
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits".into(),
            ));
        }
        Ok(Some(count as u8))
    }

    /// `TemporalDurationToString`. `precision` is the number of fractional
    /// second digits to emit, or `None` for `auto` (emit only as many as the
    /// value needs, and none at all for a whole number of seconds).
    pub(in super::super) fn format_duration_string(
        record: &blueice_ecma402::DurationRecord,
        precision: Option<u8>,
    ) -> String {
        let mut date = String::new();
        for (value, suffix) in [
            (record.years, 'Y'),
            (record.months, 'M'),
            (record.weeks, 'W'),
            (record.days, 'D'),
        ] {
            if value != 0 {
                date.push_str(&value.unsigned_abs().to_string());
                date.push(suffix);
            }
        }
        let mut time = String::new();
        for (value, suffix) in [(record.hours, 'H'), (record.minutes, 'M')] {
            if value != 0 {
                time.push_str(&value.unsigned_abs().to_string());
                time.push(suffix);
            }
        }
        // Seconds and every sub-second field are one exact quantity: 1,500
        // milliseconds serializes as `1.5S`, and 9,007,199,254,740,991
        // milliseconds must not lose precision on the way there.
        let subsecond_total = record.seconds * 1_000_000_000
            + record.milliseconds * 1_000_000
            + record.microseconds * 1_000
            + record.nanoseconds;
        let seconds = subsecond_total / 1_000_000_000;
        let fraction = (subsecond_total % 1_000_000_000).unsigned_abs();
        let only_seconds = date.is_empty() && time.is_empty();
        if seconds != 0 || fraction != 0 || only_seconds || precision.is_some() {
            time.push_str(&seconds.unsigned_abs().to_string());
            let digits = format!("{fraction:09}");
            match precision {
                None if fraction != 0 => {
                    time.push('.');
                    time.push_str(digits.trim_end_matches('0'));
                }
                Some(count) if count > 0 => {
                    time.push('.');
                    time.push_str(&digits[..usize::from(count)]);
                }
                _ => {}
            }
            time.push('S');
        }
        let mut result = String::new();
        if record.sign() < 0 {
            result.push('-');
        }
        result.push('P');
        result.push_str(&date);
        if !time.is_empty() {
            result.push('T');
            result.push_str(&time);
        }
        result
    }

    pub(in super::super) fn temporal_duration_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let options = self.temporal_duration_options(options)?;
            let digits = self.temporal_duration_fractional_digits(&options)?;
            let mode =
                self.temporal_rounding_mode(&options, blueice_ecma402::NumberRoundingMode::Trunc)?;
            let smallest = self.temporal_duration_unit_option(&options, "smallestUnit", false)?;
            // `ToSecondsStringPrecisionRecord`: a smallestUnit pins both the
            // emitted digit count and the rounding unit; a digit count alone
            // pins the digits and derives a unit plus increment from them.
            let (precision, unit, increment) = match smallest.unit() {
                Some(rounding::TemporalUnit::Second) => {
                    (Some(0), rounding::TemporalUnit::Second, 1)
                }
                Some(rounding::TemporalUnit::Millisecond) => {
                    (Some(3), rounding::TemporalUnit::Millisecond, 1)
                }
                Some(rounding::TemporalUnit::Microsecond) => {
                    (Some(6), rounding::TemporalUnit::Microsecond, 1)
                }
                Some(rounding::TemporalUnit::Nanosecond) => {
                    (Some(9), rounding::TemporalUnit::Nanosecond, 1)
                }
                Some(_) => {
                    return Err(RuntimeError::RangeError(
                        "Temporal.Duration.prototype.toString accepts a smallestUnit of second or \
                         smaller"
                            .into(),
                    ));
                }
                None => match digits {
                    None => (None, rounding::TemporalUnit::Nanosecond, 1),
                    Some(0) => (Some(0), rounding::TemporalUnit::Second, 1),
                    Some(count @ 1..=3) => (
                        Some(count),
                        rounding::TemporalUnit::Millisecond,
                        10_i128.pow(u32::from(3 - count)),
                    ),
                    Some(count @ 4..=6) => (
                        Some(count),
                        rounding::TemporalUnit::Microsecond,
                        10_i128.pow(u32::from(6 - count)),
                    ),
                    Some(count) => (
                        Some(count),
                        rounding::TemporalUnit::Nanosecond,
                        10_i128.pow(u32::from(9 - count)),
                    ),
                },
            };
            if unit == rounding::TemporalUnit::Nanosecond && increment == 1 {
                // Nothing to round: serialize the record exactly as stored,
                // which is what keeps a maximal seconds-plus-nanoseconds pair
                // in range instead of balancing it out of range.
                return Ok(Value::String(
                    Self::format_duration_string(&record, precision).into(),
                ));
            }
            // Rounding the time part can carry into `days`, but never past
            // them: `largestUnit` here is the duration's own largest unit (at
            // least `second`), and the date fields are carried through
            // untouched.
            let largest =
                Self::temporal_duration_largest_unit(&record).max(rounding::TemporalUnit::Second);
            let step = unit
                .nanoseconds()
                .expect("second and smaller units have an exact length")
                * increment;
            let balanced = duration_math::TimeDuration::from_fields(
                record.hours,
                record.minutes,
                record.seconds,
                record.milliseconds,
                record.microseconds,
                record.nanoseconds,
            )
            .rounded_to_step(step, mode)
            .balance_with_days(largest.min(rounding::TemporalUnit::Day));
            let rounded = Self::temporal_duration_record([
                record.years,
                record.months,
                record.weeks,
                record.days + balanced[0],
                balanced[1],
                balanced[2],
                balanced[3],
                balanced[4],
                balanced[5],
                balanced[6],
            ])?;
            Ok(Value::String(
                Self::format_duration_string(&rounded, precision).into(),
            ))
        })();
        self.stack.truncate(base);
        result
    }

    /// ECMA-402's `Temporal.Duration.prototype.toLocaleString`: build an
    /// `Intl.DurationFormat` from the same `(locales, options)` arguments and
    /// format this duration with it, rather than returning the ISO string
    /// ECMA-262's own non-402 definition would.
    pub(in super::super) fn temporal_duration_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let record = self.temporal_duration_receiver(receiver)?;
        let base = self.stack.len();
        let result = (|| {
            let formatter = self.duration_format_for_locale_string(args)?;
            self.stack.push(formatter.clone());
            let duration =
                self.alloc_temporal_value(Self::temporal_duration_value(record), false)?;
            self.stack.push(duration.clone());
            self.duration_format_format(&formatter, &duration)
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn temporal_duration_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.Duration cannot be converted to a primitive value".into(),
        ))
    }
}
