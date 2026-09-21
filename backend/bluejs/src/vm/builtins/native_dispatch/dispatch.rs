// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super::super) fn native_call(
        &mut self,
        function: NativeFunction,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if receiver
            .object_id()
            .is_some_and(|id| self.test262_foreign_reference(id).is_some())
            && matches!(
                function,
                NativeFunction::ArrayIteratorNext
                    | NativeFunction::IteratorNext
                    | NativeFunction::RegExpIteratorNext
            )
        {
            return self.test262_foreign_next(&receiver, &args);
        }
        if receiver
            .object_id()
            .is_some_and(|id| self.test262_foreign_reference(id).is_some())
            && (matches!(
                function,
                NativeFunction::TypedArrayBuffer
                    | NativeFunction::TypedArrayByteLength
                    | NativeFunction::TypedArrayByteOffset
                    | NativeFunction::TypedArrayLength
                    | NativeFunction::TypedArraySet
                    | NativeFunction::TypedArraySubarray
                    | NativeFunction::TypedArraySpecies
                    | NativeFunction::TypedArrayToStringTag
                    | NativeFunction::TypedArrayIterator(_)
            ) || matches!(
                function,
                NativeFunction::TypedArrayMethod(method) if method != TypedArrayMethod::ToLocaleString
            ))
        {
            return self
                .test262_foreign_typed_array_native_call(function, receiver, args, construct);
        }
        if matches!(
            function,
            NativeFunction::Atomics(_)
                | NativeFunction::AtomicsNotify
                | NativeFunction::AtomicsWait
                | NativeFunction::AtomicsWaitAsync
        ) {
            if let Some(result) = self.test262_foreign_atomics_call(function, &args)? {
                return Ok(result);
            }
        }
        // `RequireInternalSlot(this, ...)` is every Temporal prototype
        // member's first step, ahead of any argument access.
        if let Some(kind) = function.temporal_receiver_kind() {
            self.require_temporal_receiver(&receiver, kind)?;
        }
        let first = native::argument(&args, 0);
        match function {
            NativeFunction::Promise => self.promise_constructor(first.clone(), construct),
            NativeFunction::PromiseResolvingFunction { promise, fulfill } => {
                if fulfill {
                    self.resolve_promise(promise, first.clone())?;
                } else {
                    self.settle_promise(promise, PromiseStatus::Rejected(first.clone()))?;
                }
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseCapabilityExecutor { storage } => {
                let resolve = native::argument(&args, 0).clone();
                let reject = native::argument(&args, 1).clone();
                self.with_roots(|heap| heap.set(storage, "resolve", resolve))?;
                self.with_roots(|heap| heap.set(storage, "reject", reject))?;
                Ok(Value::Undefined)
            }
            NativeFunction::AsyncFromSyncFulfill { target, done } => {
                let result = self.iterator_result(first.clone(), done);
                match result {
                    Ok(result) => self.settle_promise(target, PromiseStatus::Fulfilled(result))?,
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                }
                Ok(Value::Undefined)
            }
            NativeFunction::AsyncFromSyncReject { target, record } => {
                // AsyncFromSyncIteratorContinuation closes with an existing
                // throw completion. IteratorClose must retain that original
                // rejection even when the delegate's return method fails or
                // returns a non-object.
                let _ = self.iterator_close(&Value::Object(record));
                self.settle_promise(target, PromiseStatus::Rejected(first.clone()))?;
                Ok(Value::Undefined)
            }
            NativeFunction::AbstractModuleSource => Err(RuntimeError::TypeError(
                "AbstractModuleSource is an abstract constructor".into(),
            )),
            NativeFunction::AbstractModuleSourceToStringTag => Ok(Value::Undefined),
            NativeFunction::Function => self.function_constructor(&args),
            NativeFunction::AsyncFunction => self.async_function_constructor(&args),
            NativeFunction::Error(name) => self.error_constructor(name, &args, construct),
            NativeFunction::ErrorToString => self.error_to_string(&receiver),
            NativeFunction::Test262(name) => self.test262_call(name, &args),
            NativeFunction::Test262Done => {
                self.test262_done = Some(if matches!(first, Value::Undefined) {
                    Ok(())
                } else {
                    Err(first.clone())
                });
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseThen => self.promise_then(&receiver, &args),
            NativeFunction::PromiseCatch => self.promise_catch(&receiver, first),
            NativeFunction::PromiseFinally => self.promise_finally(&receiver, first),
            NativeFunction::PromiseResolve => {
                self.promise_resolve_constructor(&receiver, first.clone())
            }
            NativeFunction::PromiseReject => self.promise_reject(first.clone()),
            NativeFunction::PromiseAll => self.promise_all(&receiver, first),
            NativeFunction::PromiseRace => self.promise_race(&receiver, first),
            NativeFunction::PromiseAny => self.promise_any(&receiver, first),
            NativeFunction::PromiseAllSettled => self.promise_all_settled_static(&receiver, first),
            NativeFunction::PromiseAllResolve { target, index } => {
                self.promise_all_settled(target, index, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseAllReject { target } => {
                self.promise_all_reject(target, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseRaceFulfill { target } => {
                self.settle_promise(target, PromiseStatus::Fulfilled(first.clone()))?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseRaceReject { target } => {
                self.settle_promise(target, PromiseStatus::Rejected(first.clone()))?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseAnyFulfill { target } => {
                self.promise_any_fulfill(target, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseAnyReject { target, index } => {
                self.promise_any_reject(target, index, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseAllSettledFulfill { target, index } => {
                self.promise_all_settled_result(target, index, first.clone(), true)?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseAllSettledReject { target, index } => {
                self.promise_all_settled_result(target, index, first.clone(), false)?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseWithResolvers => self.promise_with_resolvers(),
            NativeFunction::ToLocaleLowerCase
            | NativeFunction::ToLocaleUpperCase
            | NativeFunction::LocaleCompare => {
                let string = self.string_receiver(&receiver)?;
                if function == NativeFunction::LocaleCompare {
                    let other = self.coerce_string(first)?;
                    let collator = self
                        .resolve_collator(native::argument(&args, 1), native::argument(&args, 2))?;
                    Ok(crate::intl::collate(&collator, &string, &other))
                } else {
                    let locales = self.canonical_locales(first)?;
                    let locale = locales
                        .first()
                        .map(|locale| locale.locale().clone())
                        .unwrap_or(icu_locale_core::locale!("en-US"));
                    crate::intl::case_map(
                        &string,
                        &locale,
                        function == NativeFunction::ToLocaleUpperCase,
                        self.config.max_string_bytes,
                    )
                    .map(Value::String)
                }
            }
            NativeFunction::Collator => self.create_collator(&args, construct),
            NativeFunction::IntlService(service) => {
                self.create_intl_service(service, &receiver, &args, construct)
            }
            NativeFunction::Locale => self.create_locale(&args, construct),
            NativeFunction::CanonicalLocales => {
                let locales = self.canonical_locales(first)?;
                self.array_from(
                    locales
                        .into_iter()
                        .map(|l| Value::String(l.to_string().into()))
                        .collect(),
                )
            }
            NativeFunction::SupportedValuesOf => self.supported_values_of(first),
            NativeFunction::SupportedLocales => self.supported_locales(&args),
            NativeFunction::CollatorCompareGetter => self.collator_compare_getter(&receiver),
            NativeFunction::CollatorCompare => {
                let collator = self.collator_data(&receiver)?;
                let left = self.coerce_string(first)?;
                let right = self.coerce_string(native::argument(&args, 1))?;
                Ok(crate::intl::collate(&collator, &left, &right))
            }
            NativeFunction::CollatorResolvedOptions => self.collator_resolved_options(&receiver),
            NativeFunction::DisplayNamesSupportedLocales => {
                self.display_names_supported_locales(&args)
            }
            NativeFunction::DisplayNamesOf => self.display_names_of(&receiver, first),
            NativeFunction::DisplayNamesResolvedOptions => {
                self.display_names_resolved_options(&receiver)
            }
            NativeFunction::DurationFormatSupportedLocales => {
                self.duration_format_supported_locales(&args)
            }
            NativeFunction::DurationFormatFormat => self.duration_format_format(&receiver, first),
            NativeFunction::DurationFormatFormatToParts => {
                self.duration_format_format_to_parts(&receiver, first)
            }
            NativeFunction::DurationFormatResolvedOptions => {
                self.duration_format_resolved_options(&receiver)
            }
            NativeFunction::NumberFormatSupportedLocales => {
                self.number_format_supported_locales(&args)
            }
            NativeFunction::NumberFormatFormatGetter => self.number_format_format_getter(&receiver),
            NativeFunction::NumberFormatFormat => self.number_format_format(&receiver, first),
            NativeFunction::NumberFormatFormatToParts => {
                self.number_format_format_to_parts(&receiver, first)
            }
            NativeFunction::NumberFormatFormatRange => {
                self.number_format_format_range(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::NumberFormatFormatRangeToParts => self
                .number_format_format_range_to_parts(&receiver, first, native::argument(&args, 1)),
            NativeFunction::NumberFormatResolvedOptions => {
                self.number_format_resolved_options(&receiver)
            }
            NativeFunction::DateTimeFormatSupportedLocales => {
                self.date_time_format_supported_locales(&args)
            }
            NativeFunction::DateTimeFormatFormatGetter => {
                self.date_time_format_format_getter(&receiver)
            }
            NativeFunction::DateTimeFormatFormat => self.date_time_format_format(&receiver, first),
            NativeFunction::DateTimeFormatFormatToParts => {
                self.date_time_format_format_to_parts(&receiver, first)
            }
            NativeFunction::DateTimeFormatFormatRange => {
                self.date_time_format_format_range(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::DateTimeFormatFormatRangeToParts => self
                .date_time_format_format_range_to_parts(
                    &receiver,
                    first,
                    native::argument(&args, 1),
                ),
            NativeFunction::DateTimeFormatResolvedOptions => {
                self.date_time_format_resolved_options(&receiver)
            }
            NativeFunction::TemporalConstructor(kind) => {
                self.temporal_constructor(kind, &args, construct)
            }
            NativeFunction::TemporalFrom(kind) => {
                self.temporal_from(kind, first, native::argument(&args, 1))
            }
            NativeFunction::TemporalWithCalendar(_) => {
                self.temporal_with_calendar(&receiver, first)
            }
            NativeFunction::TemporalPlainToZonedDateTime(_) => {
                self.temporal_plain_to_zoned_date_time(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::TemporalInstantToZonedDateTimeIso => {
                self.temporal_instant_to_zoned_date_time_iso(&receiver, first)
            }
            NativeFunction::TemporalGetter(_, getter) => self.temporal_getter(&receiver, getter),
            NativeFunction::TemporalZonedDateTimeToLocaleString => {
                self.temporal_zoned_date_time_to_locale_string(&receiver, &args)
            }
            NativeFunction::TemporalInstantAdd => {
                self.temporal_instant_add(&receiver, first, false)
            }
            NativeFunction::TemporalInstantSubtract => {
                self.temporal_instant_add(&receiver, first, true)
            }
            NativeFunction::TemporalInstantRound => self.temporal_instant_round(&receiver, first),
            NativeFunction::TemporalInstantUntil => self.temporal_instant_difference(
                &receiver,
                first,
                native::argument(&args, 1),
                false,
            ),
            NativeFunction::TemporalInstantSince => {
                self.temporal_instant_difference(&receiver, first, native::argument(&args, 1), true)
            }
            NativeFunction::TemporalInstantEquals => self.temporal_instant_equals(&receiver, first),
            NativeFunction::TemporalInstantCompare => {
                self.temporal_instant_compare(first, native::argument(&args, 1))
            }
            NativeFunction::TemporalInstantToString => {
                self.temporal_instant_to_string(&receiver, first)
            }
            NativeFunction::TemporalInstantToLocaleString => {
                self.temporal_instant_to_locale_string(&receiver, &args)
            }
            NativeFunction::TemporalInstantToJson => {
                self.temporal_instant_to_string(&receiver, &Value::Undefined)
            }
            NativeFunction::TemporalInstantValueOf => self.temporal_instant_value_of(),
            NativeFunction::TemporalFromEpochMilliseconds => {
                self.temporal_from_epoch_milliseconds(first)
            }
            NativeFunction::TemporalFromEpochNanoseconds => {
                self.temporal_from_epoch_nanoseconds(first)
            }
            NativeFunction::TemporalPlainTimeAdd => {
                self.temporal_plain_time_add(&receiver, first, false)
            }
            NativeFunction::TemporalPlainTimeSubtract => {
                self.temporal_plain_time_add(&receiver, first, true)
            }
            NativeFunction::TemporalPlainTimeRound => {
                self.temporal_plain_time_round(&receiver, first)
            }
            NativeFunction::TemporalPlainTimeUntil => self.temporal_plain_time_difference(
                &receiver,
                first,
                native::argument(&args, 1),
                false,
            ),
            NativeFunction::TemporalPlainTimeSince => self.temporal_plain_time_difference(
                &receiver,
                first,
                native::argument(&args, 1),
                true,
            ),
            NativeFunction::TemporalPlainTimeEquals => {
                self.temporal_plain_time_equals(&receiver, first)
            }
            NativeFunction::TemporalPlainTimeCompare => {
                self.temporal_plain_time_compare(first, native::argument(&args, 1))
            }
            NativeFunction::TemporalPlainTimeWith => {
                self.temporal_plain_time_with(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::TemporalPlainTimeToString => {
                self.temporal_plain_time_to_string(&receiver, first)
            }
            NativeFunction::TemporalPlainTimeToJson => {
                self.temporal_plain_time_to_string(&receiver, &Value::Undefined)
            }
            NativeFunction::TemporalPlainTimeToLocaleString => {
                self.temporal_plain_time_to_locale_string(&receiver, &args)
            }
            NativeFunction::TemporalPlainTimeValueOf => self.temporal_plain_time_value_of(),
            NativeFunction::TemporalNowInstant => self.temporal_now_instant(),
            NativeFunction::TemporalNowTimeZoneId => self.temporal_now_time_zone_id(),
            NativeFunction::TemporalNowPlainDateIso => {
                self.temporal_now_plain(TemporalKind::PlainDate, first)
            }
            NativeFunction::TemporalNowPlainDateTimeIso => {
                self.temporal_now_plain(TemporalKind::PlainDateTime, first)
            }
            NativeFunction::TemporalNowPlainTimeIso => {
                self.temporal_now_plain(TemporalKind::PlainTime, first)
            }
            NativeFunction::TemporalNowZonedDateTimeIso => self.temporal_now_zoned_date_time(first),
            NativeFunction::TemporalDurationWith => self.temporal_duration_with(&receiver, first),
            NativeFunction::TemporalDurationNegated => {
                self.temporal_duration_negated(&receiver, false)
            }
            NativeFunction::TemporalDurationAbs => self.temporal_duration_negated(&receiver, true),
            NativeFunction::TemporalDurationAdd => {
                self.temporal_duration_add(&receiver, first, false)
            }
            NativeFunction::TemporalDurationSubtract => {
                self.temporal_duration_add(&receiver, first, true)
            }
            NativeFunction::TemporalDurationRound => self.temporal_duration_round(&receiver, first),
            NativeFunction::TemporalDurationTotal => self.temporal_duration_total(&receiver, first),
            NativeFunction::TemporalDurationCompare => self.temporal_duration_compare(
                first,
                native::argument(&args, 1),
                native::argument(&args, 2),
            ),
            NativeFunction::TemporalDurationToString => {
                self.temporal_duration_to_string(&receiver, first)
            }
            NativeFunction::TemporalDurationToJson => {
                self.temporal_duration_to_string(&receiver, &Value::Undefined)
            }
            NativeFunction::TemporalDurationToLocaleString => {
                self.temporal_duration_to_locale_string(&receiver, &args)
            }
            NativeFunction::TemporalDurationValueOf => self.temporal_duration_value_of(),
            NativeFunction::TemporalDateWith(_) => {
                self.temporal_date_with(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::TemporalDateAdd(_) => {
                self.temporal_date_add(&receiver, first, native::argument(&args, 1), false)
            }
            NativeFunction::TemporalDateSubtract(_) => {
                self.temporal_date_add(&receiver, first, native::argument(&args, 1), true)
            }
            NativeFunction::TemporalDateUntil(_) => {
                self.temporal_date_difference(&receiver, first, native::argument(&args, 1), false)
            }
            NativeFunction::TemporalDateSince(_) => {
                self.temporal_date_difference(&receiver, first, native::argument(&args, 1), true)
            }
            NativeFunction::TemporalDateEquals(_) => self.temporal_date_equals(&receiver, first),
            NativeFunction::TemporalDateCompare(kind) => {
                self.temporal_date_compare(kind, first, native::argument(&args, 1))
            }
            NativeFunction::TemporalDateToString(_) => {
                self.temporal_date_to_string(&receiver, first)
            }
            NativeFunction::TemporalDateToJson(_) => {
                self.temporal_date_to_string(&receiver, &Value::Undefined)
            }
            NativeFunction::TemporalDateToLocaleString(_) => {
                self.temporal_date_to_locale_string(&receiver, &args)
            }
            NativeFunction::TemporalDateValueOf => self.temporal_date_value_of(),
            NativeFunction::TemporalPlainDateToPlainDateTime => {
                self.temporal_plain_date_to_plain_date_time(&receiver, first)
            }
            NativeFunction::TemporalPlainDateToPlainYearMonth => {
                self.temporal_plain_date_to_plain_year_month(&receiver)
            }
            NativeFunction::TemporalPlainDateToPlainMonthDay => {
                self.temporal_plain_date_to_plain_month_day(&receiver)
            }
            NativeFunction::TemporalPlainDateTimeToPlainDate => {
                self.temporal_plain_date_time_to_plain_date(&receiver)
            }
            NativeFunction::TemporalPlainDateTimeToPlainTime => {
                self.temporal_plain_date_time_to_plain_time(&receiver)
            }
            NativeFunction::TemporalPlainDateTimeWithPlainTime => {
                self.temporal_plain_date_time_with_plain_time(&receiver, first)
            }
            NativeFunction::TemporalPlainDateTimeRound => {
                self.temporal_plain_date_time_round(&receiver, first)
            }
            NativeFunction::TemporalYearMonthWith => {
                self.temporal_year_month_with(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::TemporalYearMonthAdd => {
                self.temporal_year_month_add(&receiver, first, native::argument(&args, 1), false)
            }
            NativeFunction::TemporalYearMonthSubtract => {
                self.temporal_year_month_add(&receiver, first, native::argument(&args, 1), true)
            }
            NativeFunction::TemporalYearMonthUntil => self.temporal_year_month_difference(
                &receiver,
                first,
                native::argument(&args, 1),
                false,
            ),
            NativeFunction::TemporalYearMonthSince => self.temporal_year_month_difference(
                &receiver,
                first,
                native::argument(&args, 1),
                true,
            ),
            NativeFunction::TemporalYearMonthEquals => {
                self.temporal_year_month_equals(&receiver, first)
            }
            NativeFunction::TemporalYearMonthCompare => {
                self.temporal_year_month_compare(first, native::argument(&args, 1))
            }
            NativeFunction::TemporalYearMonthToString => {
                self.temporal_year_month_to_string(&receiver, first)
            }
            NativeFunction::TemporalYearMonthToJson => {
                self.temporal_year_month_to_string(&receiver, &Value::Undefined)
            }
            NativeFunction::TemporalYearMonthToLocaleString => {
                self.temporal_year_month_to_locale_string(&receiver, &args)
            }
            NativeFunction::TemporalYearMonthValueOf => self.temporal_year_month_value_of(),
            NativeFunction::TemporalYearMonthToPlainDate => {
                self.temporal_year_month_to_plain_date(&receiver, first)
            }
            NativeFunction::TemporalMonthDayWith => {
                self.temporal_month_day_with(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::TemporalMonthDayEquals => {
                self.temporal_month_day_equals(&receiver, first)
            }
            NativeFunction::TemporalMonthDayToString => {
                self.temporal_month_day_to_string(&receiver, first)
            }
            NativeFunction::TemporalMonthDayToJson => {
                self.temporal_month_day_to_string(&receiver, &Value::Undefined)
            }
            NativeFunction::TemporalMonthDayToLocaleString => {
                self.temporal_month_day_to_locale_string(&receiver, &args)
            }
            NativeFunction::TemporalMonthDayValueOf => self.temporal_month_day_value_of(),
            NativeFunction::TemporalMonthDayToPlainDate => {
                self.temporal_month_day_to_plain_date(&receiver, first)
            }
            NativeFunction::TemporalZonedDateTimeWith => {
                self.temporal_zoned_date_time_with(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::TemporalZonedDateTimeWithTimeZone => {
                self.temporal_zoned_date_time_with_time_zone(&receiver, first)
            }
            NativeFunction::TemporalZonedDateTimeWithPlainTime => {
                self.temporal_zoned_date_time_with_plain_time(&receiver, first)
            }
            NativeFunction::TemporalZonedDateTimeAdd => self.temporal_zoned_date_time_add(
                &receiver,
                first,
                native::argument(&args, 1),
                false,
            ),
            NativeFunction::TemporalZonedDateTimeSubtract => self.temporal_zoned_date_time_add(
                &receiver,
                first,
                native::argument(&args, 1),
                true,
            ),
            NativeFunction::TemporalZonedDateTimeRound => {
                self.temporal_zoned_date_time_round(&receiver, first)
            }
            NativeFunction::TemporalZonedDateTimeUntil => self.temporal_zoned_date_time_difference(
                &receiver,
                first,
                native::argument(&args, 1),
                false,
            ),
            NativeFunction::TemporalZonedDateTimeSince => self.temporal_zoned_date_time_difference(
                &receiver,
                first,
                native::argument(&args, 1),
                true,
            ),
            NativeFunction::TemporalZonedDateTimeEquals => {
                self.temporal_zoned_date_time_equals(&receiver, first)
            }
            NativeFunction::TemporalZonedDateTimeCompare => {
                self.temporal_zoned_date_time_compare(first, native::argument(&args, 1))
            }
            NativeFunction::TemporalZonedDateTimeToString => {
                self.temporal_zoned_date_time_to_string(&receiver, first)
            }
            NativeFunction::TemporalZonedDateTimeToJson => {
                self.temporal_zoned_date_time_to_string(&receiver, &Value::Undefined)
            }
            NativeFunction::TemporalZonedDateTimeValueOf => {
                self.temporal_zoned_date_time_value_of()
            }
            NativeFunction::TemporalZonedDateTimeToInstant => {
                self.temporal_zoned_date_time_to_instant(&receiver)
            }
            NativeFunction::TemporalZonedDateTimeToPlainDate => {
                self.temporal_zoned_date_time_to_plain_date(&receiver)
            }
            NativeFunction::TemporalZonedDateTimeToPlainTime => {
                self.temporal_zoned_date_time_to_plain_time(&receiver)
            }
            NativeFunction::TemporalZonedDateTimeToPlainDateTime => {
                self.temporal_zoned_date_time_to_plain_date_time(&receiver)
            }
            NativeFunction::TemporalZonedDateTimeStartOfDay => {
                self.temporal_zoned_date_time_start_of_day(&receiver)
            }
            NativeFunction::TemporalZonedDateTimeGetTimeZoneTransition => {
                self.temporal_zoned_date_time_get_time_zone_transition(&receiver, first)
            }
            NativeFunction::ListFormatSupportedLocales => self.list_format_supported_locales(&args),
            NativeFunction::ListFormatFormat => self.list_format_format(&receiver, first),
            NativeFunction::ListFormatFormatToParts => {
                self.list_format_format_to_parts(&receiver, first)
            }
            NativeFunction::ListFormatResolvedOptions => {
                self.list_format_resolved_options(&receiver)
            }
            NativeFunction::PluralRulesSupportedLocales => {
                self.plural_rules_supported_locales(&args)
            }
            NativeFunction::PluralRulesSelect => self.plural_rules_select(&receiver, first),
            NativeFunction::PluralRulesSelectRange => {
                self.plural_rules_select_range(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::PluralRulesResolvedOptions => {
                self.plural_rules_resolved_options(&receiver)
            }
            NativeFunction::SegmenterSupportedLocales => self.segmenter_supported_locales(&args),
            NativeFunction::SegmenterResolvedOptions => self.segmenter_resolved_options(&receiver),
            NativeFunction::SegmenterSegment => self.segmenter_segment(&receiver, first),
            NativeFunction::SegmentsContaining => self.segments_containing(&receiver, first),
            NativeFunction::SegmentsIterator => self.segments_iterator(&receiver),
            NativeFunction::SegmentIteratorNext => self.segment_iterator_next(&receiver),
            NativeFunction::RelativeTimeFormatSupportedLocales => {
                self.relative_time_format_supported_locales(&args)
            }
            NativeFunction::RelativeTimeFormatFormat => {
                self.relative_time_format_format(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::RelativeTimeFormatFormatToParts => {
                self.relative_time_format_to_parts(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::RelativeTimeFormatResolvedOptions => {
                self.relative_time_format_resolved_options(&receiver)
            }
            NativeFunction::LocaleToString => self.locale_to_string(&receiver),
            NativeFunction::LocaleMaximize => self.locale_transform(&receiver, true),
            NativeFunction::LocaleMinimize => self.locale_transform(&receiver, false),
            NativeFunction::LocaleGetter(name) => self.locale_getter(&receiver, name),
            NativeFunction::LocaleInfo(name) => self.locale_info(&receiver, name),
            NativeFunction::Array => {
                let prototype = if construct {
                    self.constructor_prototype(self.array_prototype)?
                } else {
                    self.array_prototype
                };
                if args.len() == 1 {
                    if let Value::Number(length) = first {
                        let Value::Number(length) =
                            self.array_length_value(&Value::Number(*length))?
                        else {
                            unreachable!()
                        };
                        return Ok(Value::Object(self.with_roots(|heap| {
                            heap.alloc_array(length as u32, Some(prototype))
                        })?));
                    }
                }
                self.array_from_with_prototype(args, prototype)
            }
            NativeFunction::Date => self.date_constructor(&args, construct),
            NativeFunction::DateNow => Ok(Value::Number(Self::current_time())),
            NativeFunction::DateParse => self.date_parse(first),
            NativeFunction::DateUtc => self.date_utc(&args),
            NativeFunction::DateMethod(method) => self.date_method(method, &receiver, &args),
            NativeFunction::ArrayBuffer => self.array_buffer_constructor(&args, construct),
            NativeFunction::ArrayBufferByteLength => Ok(Value::Number(
                self.heap
                    .array_buffer_byte_length(self.array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::ArrayBufferDetached => Ok(Value::Bool(
                self.heap
                    .array_buffer_is_detached(self.array_buffer_receiver(&receiver)?)?,
            )),
            NativeFunction::ArrayBufferMaxByteLength => Ok(Value::Number(
                self.heap
                    .buffer_max_byte_length(self.array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            // IsResizableArrayBuffer looks only at the buffer's kind, so a
            // detached resizable buffer still reports true.
            NativeFunction::ArrayBufferResizable => Ok(Value::Bool(
                self.heap
                    .array_buffer_is_resizable(self.array_buffer_receiver(&receiver)?)?,
            )),
            NativeFunction::ArrayBufferResize => self.buffer_resize(&receiver, first),
            NativeFunction::ArrayBufferTransfer => {
                self.array_buffer_transfer(&receiver, &args, false)
            }
            NativeFunction::ArrayBufferTransferToFixedLength => {
                self.array_buffer_transfer(&receiver, &args, true)
            }
            NativeFunction::ArrayBufferSlice => self.array_buffer_slice(&receiver, &args),
            NativeFunction::ArrayBufferImmutable => self.array_buffer_immutable(&receiver),
            NativeFunction::ArrayBufferTransferToImmutable => {
                self.array_buffer_transfer_to_immutable(&receiver, &args)
            }
            NativeFunction::ArrayBufferSliceToImmutable => {
                self.array_buffer_slice_to_immutable(&receiver, &args)
            }
            NativeFunction::ArrayBufferIsView => {
                Ok(Value::Bool(first.object_id().is_some_and(|object| {
                    self.heap.is_data_view(object).unwrap_or(false)
                        || self.heap.is_typed_array(object).unwrap_or(false)
                })))
            }
            NativeFunction::ArrayBufferSpecies => Ok(receiver),
            NativeFunction::SharedArrayBuffer => {
                self.shared_array_buffer_constructor(&args, construct)
            }
            NativeFunction::SharedArrayBufferByteLength => Ok(Value::Number(
                self.heap
                    .buffer_byte_length(self.shared_array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::SharedArrayBufferMaxByteLength => Ok(Value::Number(
                self.heap
                    .buffer_max_byte_length(self.shared_array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::SharedArrayBufferGrowable => Ok(Value::Bool(
                self.heap
                    .buffer_growable(self.shared_array_buffer_receiver(&receiver)?)?,
            )),
            NativeFunction::SharedArrayBufferGrow => self.shared_buffer_grow(&receiver, first),
            NativeFunction::SharedArrayBufferSlice => {
                self.shared_array_buffer_slice(&receiver, &args)
            }
            NativeFunction::SharedArrayBufferSpecies => Ok(receiver),
            NativeFunction::Atomics(operation) => self.atomics_operation(&args, operation),
            NativeFunction::AtomicsIsLockFree => self.atomics_is_lock_free(first),
            NativeFunction::AtomicsNotify => self.atomics_notify(&args),
            NativeFunction::AtomicsPause => Ok(Value::Undefined),
            NativeFunction::AtomicsWait => self.atomics_wait(&args),
            NativeFunction::AtomicsWaitAsync => self.atomics_wait_async(&args),
            NativeFunction::DataView => self.data_view_constructor(&args, construct),
            NativeFunction::DataViewBuffer => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError("DataView method requires a DataView receiver".into())
                })?;
                let buffer = self
                    .heap
                    .data_view_buffer(object)
                    .map_err(|error| match error {
                        HeapError::InvalidInternalSlot(_) | HeapError::InvalidObject(_) => {
                            RuntimeError::TypeError(
                                "DataView method requires a DataView receiver".into(),
                            )
                        }
                        error => error.into(),
                    })?;
                Ok(Value::Object(buffer))
            }
            NativeFunction::DataViewByteLength => {
                let (_, _, length) = self.data_view_receiver(&receiver)?;
                Ok(Value::Number(length as f64))
            }
            NativeFunction::DataViewByteOffset => {
                let (_, offset, _) = self.data_view_receiver(&receiver)?;
                Ok(Value::Number(offset as f64))
            }
            NativeFunction::DataViewGet {
                width,
                signed,
                floating,
                bigint,
            } => self.data_view_get(&receiver, &args, width, signed, floating, bigint),
            NativeFunction::DataViewSet {
                width,
                signed,
                floating,
                bigint,
            } => self.data_view_set(&receiver, &args, width, signed, floating, bigint),
            NativeFunction::TypedArray(kind) => {
                self.typed_array_constructor(&args, construct, kind)
            }
            NativeFunction::TypedArrayIntrinsic => Err(RuntimeError::TypeError(
                "%TypedArray% is not directly constructible".into(),
            )),
            NativeFunction::TypedArrayBuffer => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                // Like DataView.prototype.buffer, this accessor exposes the
                // stored [[ViewedArrayBuffer]] without validating its current
                // detached or out-of-bounds state.
                let (buffer, _, _, _) =
                    self.heap
                        .typed_array_info(object)
                        .map_err(|error| match error {
                            HeapError::InvalidInternalSlot(_) | HeapError::InvalidObject(_) => {
                                RuntimeError::TypeError(
                                    "TypedArray method requires a TypedArray receiver".into(),
                                )
                            }
                            error => error.into(),
                        })?;
                if let Some(facade) = self.test262_foreign_buffer_facade(buffer) {
                    return Ok(facade);
                }
                Ok(Value::Object(buffer))
            }
            NativeFunction::TypedArrayByteLength => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                let (buffer, _, length, kind) = self.heap.typed_array_info(object)?;
                let byte_length = if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(object)?
                {
                    0
                } else {
                    length * kind.byte_width()
                };
                Ok(Value::Number(byte_length as f64))
            }
            NativeFunction::TypedArrayByteOffset => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                let (buffer, offset, _, _) = self.heap.typed_array_info(object)?;
                let byte_offset = if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(object)?
                {
                    0
                } else {
                    offset
                };
                Ok(Value::Number(byte_offset as f64))
            }
            NativeFunction::TypedArrayLength => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                let (buffer, _, length, _) = self.heap.typed_array_info(object)?;
                let element_length = if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(object)?
                {
                    0
                } else {
                    length
                };
                Ok(Value::Number(element_length as f64))
            }
            NativeFunction::TypedArraySet => self.typed_array_set(&receiver, &args),
            NativeFunction::TypedArraySubarray => self.typed_array_subarray(&receiver, &args),
            NativeFunction::TypedArraySpecies => Ok(receiver),
            NativeFunction::TypedArrayToStringTag => Ok(match receiver.object_id() {
                Some(object) => match self.heap.typed_array_info(object) {
                    Ok((_, _, _, kind)) => Value::String(kind.name().into()),
                    Err(_) => Value::Undefined,
                },
                None => Value::Undefined,
            }),
            NativeFunction::TypedArrayFrom => self.typed_array_from(&receiver, &args),
            NativeFunction::TypedArrayOf => self.typed_array_of(&receiver, &args),
            NativeFunction::Uint8ArrayFromBase64 => self.uint8_array_from_base64(&args, construct),
            NativeFunction::Uint8ArrayFromHex => self.uint8_array_from_hex(&args, construct),
            NativeFunction::Uint8ArrayMethod(method) => {
                self.uint8_array_method(&receiver, &args, construct, method)
            }
            NativeFunction::TypedArrayIterator(kind) => {
                self.typed_array_receiver(&receiver)?;
                let object = receiver
                    .object_id()
                    .expect("validated TypedArray receiver has an object identity");
                let prototype = self.array_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_array_iterator(object, kind, prototype)
                })?))
            }
            NativeFunction::TypedArrayMethod(method) => {
                self.typed_array_method(&receiver, &args, method)
            }
            NativeFunction::Proxy => self.proxy_constructor(&args, construct),
            NativeFunction::ProxyRevocable => self.proxy_revocable(&args),
            NativeFunction::ProxyRevoker(proxy) => {
                self.with_roots(|heap| heap.revoke_proxy(proxy))?;
                Ok(Value::Undefined)
            }
            NativeFunction::Map => self.collection_constructor(true, &args, construct),
            NativeFunction::MapMethod(method) => self.map_method(method, &receiver, &args),
            NativeFunction::MapSize => {
                let Some(map) = receiver.object_id() else {
                    return Err(RuntimeError::TypeError(
                        "Map size requires a Map receiver".into(),
                    ));
                };
                if !self.heap.is_map(map)? {
                    return Err(RuntimeError::TypeError(
                        "Map size requires a Map receiver".into(),
                    ));
                }
                Ok(Value::Number(self.heap.map_size(map)? as f64))
            }
            NativeFunction::Set => self.collection_constructor(false, &args, construct),
            NativeFunction::SetMethod(method) => self.set_method(method, &receiver, &args),
            NativeFunction::SetSize => {
                let Some(set) = receiver.object_id() else {
                    return Err(RuntimeError::TypeError(
                        "Set size requires a Set receiver".into(),
                    ));
                };
                if !self.heap.is_set(set)? {
                    return Err(RuntimeError::TypeError(
                        "Set size requires a Set receiver".into(),
                    ));
                }
                Ok(Value::Number(self.heap.set_size(set)? as f64))
            }
            NativeFunction::WeakMap => self.weak_collection_constructor(true, &args, construct),
            NativeFunction::WeakSet => self.weak_collection_constructor(false, &args, construct),
            NativeFunction::WeakRef => self.weak_ref_constructor(first.clone(), construct),
            NativeFunction::WeakRefDeref => self.weak_ref_deref(&receiver),
            NativeFunction::FinalizationRegistry => {
                self.finalization_registry_constructor(first.clone(), construct)
            }
            NativeFunction::FinalizationRegistryRegister => {
                self.finalization_registry_register(&receiver, &args)
            }
            NativeFunction::FinalizationRegistryUnregister => {
                self.finalization_registry_unregister(&receiver, first.clone())
            }
            NativeFunction::DisposableStack { is_async } => {
                self.disposable_stack_constructor(is_async, construct)
            }
            NativeFunction::DisposableStackDispose { is_async } => {
                if is_async {
                    self.disposable_stack_dispose_async(&receiver)
                } else {
                    self.disposable_stack_dispose(&receiver)
                }
            }
            NativeFunction::DisposableStackUse { is_async } => {
                self.disposable_stack_use(&receiver, first.clone(), is_async)
            }
            NativeFunction::DisposableStackAdopt { is_async } => self.disposable_stack_adopt(
                &receiver,
                first.clone(),
                native::argument(&args, 1).clone(),
                is_async,
            ),
            NativeFunction::DisposableStackDefer { is_async } => {
                self.disposable_stack_defer(&receiver, first.clone(), is_async)
            }
            NativeFunction::DisposableStackMove { is_async } => {
                self.disposable_stack_move(&receiver, is_async)
            }
            NativeFunction::DisposableStackDisposedGetter { is_async } => {
                self.disposable_stack_disposed(&receiver, is_async)
            }
            NativeFunction::ShadowRealm => self.shadow_realm_constructor(construct),
            NativeFunction::ShadowRealmEvaluate => {
                self.shadow_realm_evaluate(receiver, first.clone())
            }
            NativeFunction::ShadowRealmImportValue => {
                let second = native::argument(&args, 1);
                self.shadow_realm_import_value(receiver, first.clone(), second.clone())
            }
            // Never reached: `dispatch_call` routes any callee registered
            // in `shadow_wrapped_functions` to `shadow_call_wrapped` before
            // a callee is ever reduced to this bare `NativeFunction` tag.
            NativeFunction::ShadowRealmWrappedFunction => Err(RuntimeError::TypeError(
                "ShadowRealm wrapped function called without its membrane record".into(),
            )),
            NativeFunction::WeakCollectionMethod { map, method } => {
                self.weak_collection_method(map, method, &receiver, &args)
            }
            NativeFunction::ArrayIsArray => Ok(Value::Bool(self.is_array(first)?)),
            NativeFunction::ArrayAt => self.array_at(&receiver, first),
            NativeFunction::ArrayFill => self.array_fill(&receiver, &args),
            NativeFunction::ArrayCopyWithin => self.array_copy_within(&receiver, &args),
            NativeFunction::ArrayToReversed => self.array_to_reversed(&receiver),
            NativeFunction::ArrayToSorted => self.array_to_sorted(&receiver, first),
            NativeFunction::ArrayToSpliced => self.array_to_spliced(&receiver, &args),
            NativeFunction::ArrayWith => self.array_with(&receiver, &args),
            NativeFunction::ArrayFlat => self.array_flat(&receiver, first),
            NativeFunction::ArrayFlatMap => {
                self.array_flat_map(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayOf => self.array_of_method(&receiver, &args),
            NativeFunction::ArraySpecies => Ok(receiver),
            NativeFunction::ArrayFrom => self.array_from_method(&receiver, &args),
            NativeFunction::ArrayFromAsync => self.array_from_async(&receiver, &args),
            NativeFunction::ArrayFromAsyncResume { state, rejected } => {
                self.array_from_async_resume(state, first.clone(), rejected)?;
                Ok(Value::Undefined)
            }
            NativeFunction::ArrayForEach => {
                self.array_for_each(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayFilter => {
                self.array_filter(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayMap => {
                self.array_map(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayFind => {
                self.array_find(&receiver, first, native::argument(&args, 1), false, false)
            }
            NativeFunction::ArrayFindIndex => {
                self.array_find(&receiver, first, native::argument(&args, 1), false, true)
            }
            NativeFunction::ArrayFindLast => {
                self.array_find(&receiver, first, native::argument(&args, 1), true, false)
            }
            NativeFunction::ArrayFindLastIndex => {
                self.array_find(&receiver, first, native::argument(&args, 1), true, true)
            }
            NativeFunction::ArrayEvery => {
                self.array_every(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArraySome => {
                self.array_some(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayIncludes => {
                self.array_includes(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayReduce => self.array_reduce(&receiver, &args),
            NativeFunction::ArrayReduceRight => self.array_reduce_right(&receiver, &args),
            NativeFunction::ArrayPush => {
                let object = self.coerce_object(&receiver)?;
                // Genuine, growable arrays (whose `length` is a valid uint32
                // by construction) keep the direct element/length stores.
                // Every other receiver, including a frozen array or one with a
                // locked `length`, goes through the generic algorithm so its
                // strict Sets can throw.
                // The direct stores also require an unobservable prototype
                // chain: an inherited index setter or read-only property must
                // see the strict Set the generic algorithm performs.
                let direct = matches!(self.heap.is_array(object), Ok(true))
                    && matches!(self.heap.get(object, "length"), Ok(Value::Number(_)))
                    && matches!(self.heap.is_extensible(object), Ok(true))
                    && matches!(
                        self.heap.get_own_property_descriptor(object, "length"),
                        Ok(Some(PropertyDescriptor {
                            writable: Some(true),
                            ..
                        }))
                    )
                    && self.array_push_is_unobservable(object, args.len())?;
                if direct {
                    let array = Value::Object(object);
                    self.stack.push(array.clone());
                    let result = (|| {
                        for value in &args {
                            self.array_push(&array, value, 0)?;
                        }
                        self.heap.get(object, "length").map_err(Into::into)
                    })();
                    self.stack.pop();
                    result
                } else {
                    self.array_push_generic(object, &args)
                }
            }
            NativeFunction::ArrayPop => self.array_pop(&receiver),
            NativeFunction::ArrayShift => self.array_shift(&receiver),
            NativeFunction::ArrayUnshift => self.array_unshift(&receiver, &args),
            NativeFunction::ArrayReverse => self.array_reverse(&receiver),
            NativeFunction::ArrayIndexOf => {
                self.array_index_of(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayLastIndexOf => self.array_last_index_of(&receiver, first, &args),
            NativeFunction::ArraySlice => self.array_slice(&receiver, &args),
            NativeFunction::ArraySplice => self.array_splice(&receiver, &args),
            NativeFunction::ArraySort => self.array_sort(&receiver, first),
            NativeFunction::ArrayToLocaleString => {
                self.array_to_locale_string(&receiver, &args, false)
            }
            NativeFunction::NumberMethod(method) => self.number_method(&receiver, &args, method),
            NativeFunction::Eval => self.indirect_eval(first),
            NativeFunction::IsNaN => Ok(Value::Bool(self.coerce_number(first)?.is_nan())),
            NativeFunction::IsFinite => Ok(Value::Bool(self.coerce_number(first)?.is_finite())),
            NativeFunction::ParseInt => self.parse_int(first, native::argument(&args, 1)),
            NativeFunction::ParseFloat => self.parse_float(first),
            NativeFunction::EncodeUri { component } => self.encode_uri(first, component),
            NativeFunction::DecodeUri { component } => self.decode_uri(first, component),
            NativeFunction::Escape { decode } => self.escape_string(first, decode),
            NativeFunction::JsonParse => self.json_parse(first, args.get(1)),
            NativeFunction::JsonStringify => self.json_stringify(&args),
            NativeFunction::JsonRawJson => self.json_raw_json(first),
            NativeFunction::JsonIsRawJson => self.json_is_raw_json(first),
            NativeFunction::Math(method) => self.math_method(method, &args),
            NativeFunction::Bind => self.bind_function(receiver, &args),
            NativeFunction::HasInstance => self
                .has_instance(first.clone(), receiver, true)
                .map(Value::Bool),
            NativeFunction::RegExpEscape => self.regexp_escape(first),
            NativeFunction::ArrayIterator(kind) => {
                let object = self.coerce_object(&receiver)?;
                let prototype = self.array_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_array_iterator(object, kind, prototype)
                })?))
            }
            NativeFunction::ArrayIteratorNext => {
                let Value::Object(id) = receiver else {
                    return Err(RuntimeError::TypeError(
                        "Array iterator next requires an iterator".into(),
                    ));
                };
                let Some((object, index, done, kind)) = self.heap.array_iterator(id)? else {
                    return Err(RuntimeError::TypeError(
                        "Array iterator next requires an iterator".into(),
                    ));
                };
                if done {
                    return self.iterator_result(Value::Undefined, true);
                }
                let length = if self.heap.is_typed_array(object)? {
                    let (_, _, length, _) = self.typed_array_receiver(&Value::Object(object))?;
                    length as f64
                } else {
                    let length = self.get_property(&Value::Object(object), &"length".into())?;
                    self.coerce_length(&length)?
                };
                let done = index as f64 >= length;
                self.heap.advance_array_iterator(id, done);
                let value = if done {
                    Value::Undefined
                } else {
                    match kind {
                        ArrayIteratorKind::Keys => Value::Number(index as f64),
                        ArrayIteratorKind::Values => {
                            self.get_property(&Value::Object(object), &index.to_string().into())?
                        }
                        ArrayIteratorKind::Entries => {
                            let entry = self
                                .get_property(&Value::Object(object), &index.to_string().into())?;
                            self.array_from(vec![Value::Number(index as f64), entry])?
                        }
                    }
                };
                self.iterator_result(value, done)
            }
            NativeFunction::CollectionIteratorNext { map } => {
                self.collection_iterator_next(map, &receiver)
            }
            NativeFunction::GeneratorNext => {
                self.generator_validate(&receiver)?;
                self.generator_next(&receiver, Some(first.clone()), None)
            }
            NativeFunction::GeneratorReturn => {
                self.generator_validate(&receiver)?;
                self.generator_return(&receiver, first.clone())
            }
            NativeFunction::GeneratorThrow => {
                self.generator_validate(&receiver)?;
                self.generator_throw(&receiver, first.clone())
            }
            NativeFunction::AsyncGeneratorNext
            | NativeFunction::AsyncGeneratorReturn
            | NativeFunction::AsyncGeneratorThrow => {
                self.async_generator_request(&receiver, first.clone(), function)
            }
            NativeFunction::Apply => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError("apply requires a callable".into()));
                }
                let list = native::argument(&args, 1);
                let values = if matches!(list, Value::Null | Value::Undefined) {
                    Vec::new()
                } else {
                    self.array_like_values(list)?
                };
                self.call_native(receiver, first.clone(), values, false)
            }
            NativeFunction::ReflectApply => {
                if !self.is_callable(first)? {
                    return Err(RuntimeError::TypeError(
                        "Reflect.apply requires a callable target".into(),
                    ));
                }
                let values = self.array_like_values(native::argument(&args, 2))?;
                self.call_native(
                    first.clone(),
                    native::argument(&args, 1).clone(),
                    values,
                    false,
                )
            }
            NativeFunction::ReflectConstruct => {
                let new_target = if args.len() > 2 {
                    args[2].clone()
                } else {
                    first.clone()
                };
                if !self.is_constructor(first)? || !self.is_constructor(&new_target)? {
                    return Err(RuntimeError::TypeError(
                        "Reflect.construct requires constructors".into(),
                    ));
                }
                let values = self.array_like_values(native::argument(&args, 1))?;
                self.call_with_target(first.clone(), Value::Undefined, values, true, new_target)
            }
            NativeFunction::FunctionToString => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError(
                        "Function.toString requires a callable".into(),
                    ));
                }
                let initial_name = self
                    .heap
                    .function_initial_name(receiver.object_id().unwrap())?;
                Ok(Value::String(JsString::native_function_source(
                    initial_name,
                )))
            }
            NativeFunction::PrimitiveConstructor(boolean) => {
                let value = if boolean {
                    Value::Bool(self.to_boolean(first)?)
                } else if let Value::BigInt(value) = first {
                    Value::Number(value.to_f64().unwrap_or_else(|| {
                        if value.sign() == Sign::Minus {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }
                    }))
                } else {
                    Value::Number(if args.is_empty() {
                        0.0
                    } else {
                        self.coerce_number(first)?
                    })
                };
                if !construct {
                    return Ok(value);
                }
                let constructor = self.global(if boolean { "Boolean" } else { "Number" })?;
                let default = self
                    .get_property(&constructor, &"prototype".into())?
                    .object_id()
                    .unwrap();
                let prototype = self.constructor_prototype(default)?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_boxed_primitive(value, prototype)
                })?))
            }
            NativeFunction::NumberIsFinite => Ok(Value::Bool(
                matches!(first, Value::Number(number) if number.is_finite()),
            )),
            // Number.isNaN ( number ): a plain Number-type-and-NaN check
            // with no ToNumber coercion at all -- unlike the global
            // `isNaN`, `Number.isNaN('NaN')` is `false`.
            NativeFunction::NumberIsNaN => Ok(Value::Bool(
                matches!(first, Value::Number(number) if number.is_nan()),
            )),
            NativeFunction::NumberIsInteger => Ok(Value::Bool(
                matches!(first, Value::Number(number) if number.is_finite() && number.fract() == 0.0),
            )),
            NativeFunction::NumberIsSafeInteger => Ok(Value::Bool(
                matches!(first, Value::Number(number) if number.is_finite()
                    && number.fract() == 0.0
                    && number.abs() <= 9_007_199_254_740_991.0),
            )),
            NativeFunction::BigInt => {
                if construct {
                    return Err(RuntimeError::TypeError(
                        "BigInt is not a constructor".into(),
                    ));
                }
                // BigInt ( value ): a single ToPrimitive(value, number) call,
                // then NumberToBigInt for a Number result or ToBigInt for
                // everything else (which, given an already-primitive input,
                // performs no further observable coercion).
                let value = self.coerce_primitive(first, "number")?;
                match value {
                    Value::Number(value) if value.is_finite() && value.fract() == 0.0 => {
                        // An integral IEEE-754 Number can be much larger
                        // than i64 (up to roughly 2^1024). Convert its exact
                        // represented integer, rather than saturating a
                        // narrowing Rust cast before constructing the BigInt.
                        Ok(Value::BigInt(
                            BigInt::from_f64(value)
                                .expect("a finite integral Number converts to a BigInt"),
                        ))
                    }
                    Value::Number(_) => Err(RuntimeError::RangeError(
                        "BigInt conversion requires an integral Number".into(),
                    )),
                    value => Ok(Value::BigInt(self.coerce_bigint(&value)?)),
                }
            }
            NativeFunction::BigIntAsIntN | NativeFunction::BigIntAsUintN => {
                // 1. Let bits be ? ToIndex(bits). 2. Let bigint be ?
                // ToBigInt(bigint). Both are observable coercions, evaluated
                // in this order before any arithmetic.
                let bits = self.coerce_bigint_index(native::argument(&args, 0))?;
                let bigint = self.coerce_bigint(native::argument(&args, 1))?;
                if bits == 0 {
                    return Ok(Value::BigInt(BigInt::zero()));
                }
                // ToIndex alone permits bits up to 2**53-1; bound the actual
                // 2**bits allocation at a generous but finite size (same
                // "implementation capacity" style as bigint_shift/
                // bigint_exponentiate) rather than letting an extreme bits
                // value exhaust host memory.
                const MAX_ASINTN_BITS: usize = 1_000_000;
                if bits > MAX_ASINTN_BITS {
                    return Err(RuntimeError::RangeError(
                        "BigInt.asIntN/asUintN bit width exceeds implementation capacity".into(),
                    ));
                }
                let modulus = BigInt::one() << bits;
                // BigInt's `%` follows the dividend's sign (truncated
                // division), not the mathematical "modulo" the spec asks
                // for here; adding the modulus back for a negative result
                // maps it into the required [0, 2**bits) range.
                let mut result = &bigint % &modulus;
                if result.sign() == Sign::Minus {
                    result += &modulus;
                }
                if function == NativeFunction::BigIntAsIntN {
                    let half = BigInt::one() << (bits - 1);
                    if result >= half {
                        result -= modulus;
                    }
                }
                Ok(Value::BigInt(result))
            }
            NativeFunction::PrimitiveMethod { boolean, string } => {
                let value = if let Value::Object(id) = receiver {
                    self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                if !matches!(
                    (&value, boolean),
                    (Value::Bool(_), true) | (Value::Number(_), false)
                ) {
                    return Err(RuntimeError::TypeError(
                        "incompatible boxed primitive receiver".into(),
                    ));
                }
                if string {
                    Ok(Value::String(primitive::string(&value)?))
                } else {
                    Ok(value)
                }
            }
            NativeFunction::SymbolToString | NativeFunction::SymbolValueOf => {
                let value = if let Value::Object(id) = receiver {
                    self.heap
                        .boxed_primitive(id)?
                        .or(self.test262_foreign_boxed_primitive(id)?)
                        .unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::Symbol(symbol) = value else {
                    return Err(RuntimeError::TypeError(
                        "Symbol method requires a Symbol".into(),
                    ));
                };
                if function == NativeFunction::SymbolToString {
                    Ok(Value::String(symbol.descriptive_string()))
                } else {
                    Ok(Value::Symbol(symbol))
                }
            }
            NativeFunction::SymbolDescription => {
                let value = if let Value::Object(id) = receiver {
                    self.heap
                        .boxed_primitive(id)?
                        .or(self.test262_foreign_boxed_primitive(id)?)
                        .unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::Symbol(symbol) = value else {
                    return Err(RuntimeError::TypeError(
                        "Symbol description requires a Symbol".into(),
                    ));
                };
                Ok(symbol.description.map_or(Value::Undefined, Value::String))
            }
            NativeFunction::BigIntToString | NativeFunction::BigIntValueOf => {
                // thisBigIntValue(this value): a bare BigInt returns itself;
                // an object needs its own [[BigIntData]] slot (a cross-realm
                // wrapper's own heap is checked as a fallback, the same way
                // Symbol's methods already do above); anything else,
                // including the BigInt prototype object itself (which has
                // no such slot), is a TypeError.
                let value = if let Value::Object(id) = receiver {
                    self.heap
                        .boxed_primitive(id)?
                        .or(self.test262_foreign_boxed_primitive(id)?)
                        .unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::BigInt(value) = value else {
                    return Err(RuntimeError::TypeError(
                        "BigInt method requires a BigInt".into(),
                    ));
                };
                if function == NativeFunction::BigIntValueOf {
                    return Ok(Value::BigInt(value));
                }
                // BigInt.prototype.toString ( [ radix ] )
                let radix_arg = native::argument(&args, 0);
                let radix = if matches!(radix_arg, Value::Undefined) {
                    10
                } else {
                    let radix = self.coerce_number(radix_arg)?;
                    let radix = if radix.is_nan() { 0.0 } else { radix.trunc() };
                    if !(2.0..=36.0).contains(&radix) {
                        return Err(RuntimeError::RangeError(
                            "toString radix must be between 2 and 36".into(),
                        ));
                    }
                    radix as u32
                };
                Ok(Value::String(value.to_str_radix(radix).into()))
            }
            NativeFunction::BigIntToLocaleString => {
                let value = if let Value::Object(id) = receiver {
                    self.heap
                        .boxed_primitive(id)?
                        .or(self.test262_foreign_boxed_primitive(id)?)
                        .unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::BigInt(value) = value else {
                    return Err(RuntimeError::TypeError(
                        "BigInt method requires a BigInt".into(),
                    ));
                };
                let formatter = self.resolve_number_format(
                    native::argument(&args, 0),
                    native::argument(&args, 1),
                )?;
                formatter
                    .format_input(blueice_ecma402::NumberFormatInput::Decimal(
                        value.to_string(),
                    ))
                    .map(|formatted| Value::String(formatted.into()))
                    .map_err(|error| RuntimeError::RangeError(error.to_string()))
            }
            NativeFunction::RegExp => {
                self.regexp_constructor(first, native::argument(&args, 1), construct)
            }
            NativeFunction::RegExpMethod(method) => self.regexp_method(method, &receiver, &args),
            NativeFunction::RegExpGetter(name) => self.regexp_getter(name, &receiver),
            NativeFunction::RegExpLegacyGetter(which) => self.regexp_legacy_get(which, &receiver),
            NativeFunction::RegExpLegacySetter(which) => {
                self.regexp_legacy_set(which, &receiver, first)
            }
            NativeFunction::RegExpIteratorNext => self.regexp_iterator_next(&receiver),
            NativeFunction::ThrowTypeError => Err(RuntimeError::TypeError(
                "restricted function property".into(),
            )),
            NativeFunction::Empty => Ok(Value::Undefined),
            NativeFunction::ObjectValueOf => self.coerce_object(&receiver).map(Value::Object),
            NativeFunction::ObjectIsPrototypeOf => {
                // §20.1.3.6 tests the argument before coercing `this`.  That
                // ordering keeps primitive arguments observable as `false`,
                // even when `this` is null or undefined.
                let Value::Object(mut candidate) = first else {
                    return Ok(Value::Bool(false));
                };
                let object = self.coerce_object(&receiver)?;
                let base = self.stack.len();
                self.stack
                    .extend([Value::Object(object), Value::Object(candidate)]);
                let result = (|| {
                    while let Some(prototype) = self.object_get_prototype(candidate)? {
                        if prototype == object {
                            return Ok(Value::Bool(true));
                        }
                        candidate = prototype;
                        *self.stack.last_mut().expect("prototype-chain root") =
                            Value::Object(candidate);
                    }
                    Ok(Value::Bool(false))
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectDefineAccessor { getter } => {
                // Annex B.2.2.2/B.2.2.3: establish that the receiver and
                // accessor are usable before observing a coercible key. The
                // roots remain live while ToPropertyKey and a Proxy's
                // [[DefineOwnProperty]] trap can re-enter JavaScript.
                let object = self.coerce_object(&receiver)?;
                let key_value = native::argument(&args, 0).clone();
                let accessor = native::argument(&args, 1).clone();
                let base = self.stack.len();
                self.stack
                    .extend([Value::Object(object), key_value.clone(), accessor.clone()]);
                let result = (|| {
                    if !self.is_callable(&accessor)? {
                        return Err(RuntimeError::TypeError(
                            "legacy accessor must be callable".into(),
                        ));
                    }
                    let key = self.coerce_property_key(&key_value)?;
                    let descriptor = if getter {
                        PropertyDescriptor {
                            get: Some(accessor),
                            enumerable: Some(true),
                            configurable: Some(true),
                            ..PropertyDescriptor::default()
                        }
                    } else {
                        PropertyDescriptor {
                            set: Some(accessor),
                            enumerable: Some(true),
                            configurable: Some(true),
                            ..PropertyDescriptor::default()
                        }
                    };
                    if !self.object_define_own_property(object, key, descriptor)? {
                        return Err(RuntimeError::TypeError(
                            "cannot define legacy accessor".into(),
                        ));
                    }
                    Ok(Value::Undefined)
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectLookupAccessor { getter } => {
                // Annex B.2.2.4/B.2.2.5 deliberately use [[GetOwnProperty]]
                // and [[GetPrototypeOf]], so Proxy traps and their abrupt
                // completions cannot be skipped by an ordinary heap walk.
                let object = self.coerce_object(&receiver)?;
                let key_value = native::argument(&args, 0).clone();
                let base = self.stack.len();
                self.stack
                    .extend([Value::Object(object), key_value.clone()]);
                let result = (|| {
                    let key = self.coerce_property_key(&key_value)?;
                    let mut current = object;
                    loop {
                        if let Some(descriptor) = self.object_get_own_property(current, &key)? {
                            return Ok(if getter {
                                descriptor.get.unwrap_or(Value::Undefined)
                            } else {
                                descriptor.set.unwrap_or(Value::Undefined)
                            });
                        }
                        let Some(prototype) = self.object_get_prototype(current)? else {
                            return Ok(Value::Undefined);
                        };
                        current = prototype;
                        self.stack[base] = Value::Object(current);
                    }
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectPrototypeGetter => {
                let object = self.coerce_object(&receiver)?;
                let base = self.stack.len();
                self.stack.push(Value::Object(object));
                let result = self
                    .object_get_prototype(object)
                    .map(|prototype| prototype.map_or(Value::Null, Value::Object));
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectPrototypeSetter => {
                if matches!(receiver, Value::Null | Value::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "cannot convert null or undefined to Object".into(),
                    ));
                }
                let prototype = match native::argument(&args, 0) {
                    Value::Object(prototype) => Some(*prototype),
                    Value::Null => None,
                    _ => return Ok(Value::Undefined),
                };
                let Value::Object(object) = receiver else {
                    return Ok(Value::Undefined);
                };
                let base = self.stack.len();
                self.stack.push(Value::Object(object));
                if let Some(prototype) = prototype {
                    self.stack.push(Value::Object(prototype));
                }
                let result = (|| {
                    if !self.object_set_prototype(object, prototype)? {
                        return Err(RuntimeError::TypeError(
                            "cannot set object prototype".into(),
                        ));
                    }
                    Ok(Value::Undefined)
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectToString => {
                let tag = match &receiver {
                    Value::Undefined => "Undefined",
                    Value::Null => "Null",
                    Value::String(_) => "String",
                    // Symbol and BigInt obtain their default tag from their
                    // prototypes' @@toStringTag properties.  If user code
                    // replaces those with a non-string value, the ordinary
                    // fallback is Object rather than a hidden primitive tag.
                    Value::Symbol(_) => "Object",
                    Value::Number(_) => "Number",
                    Value::BigInt(_) => "Object",
                    Value::Bool(_) => "Boolean",
                    Value::Object(id) => {
                        // Only IsArray looks through a Proxy (and throws for a
                        // revoked one). Every other brand is an internal slot,
                        // which a Proxy does not have, so `id` itself is
                        // inspected and a Proxy is callable or plain.
                        let id = *id;
                        if self.is_array(&receiver)? {
                            "Array"
                        } else if self.heap.boxed_string(id)?.is_some() {
                            "String"
                        } else if self.heap.is_arguments(id)? {
                            "Arguments"
                        } else if self.heap.is_date(id)? {
                            "Date"
                        } else if self.is_callable(&receiver)? {
                            "Function"
                        } else if self.heap.regexp(id)?.is_some() {
                            "RegExp"
                        } else if self.heap.is_error(id)? {
                            "Error"
                        } else if let Some(value) = self.heap.boxed_primitive(id)? {
                            match value {
                                Value::Number(_) => "Number",
                                Value::Bool(_) => "Boolean",
                                Value::BigInt(_) | Value::Symbol(_) => "Object",
                                _ => "Object",
                            }
                        } else {
                            "Object"
                        }
                    }
                };
                let custom = if matches!(receiver, Value::Undefined | Value::Null) {
                    Value::Undefined
                } else {
                    self.get_property(&receiver, &JsSymbol::well_known("toStringTag").into())?
                };
                let mut result = JsString::from("[object ");
                result.push_str(&if let Value::String(custom) = custom {
                    custom
                } else {
                    tag.into()
                });
                result.push_str(&"]".into());
                Ok(Value::String(result))
            }
            NativeFunction::ObjectToLocaleString => {
                if matches!(receiver, Value::Null | Value::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "cannot convert null or undefined to Object".into(),
                    ));
                }
                let base = self.stack.len();
                self.stack.push(receiver.clone());
                let result = (|| {
                    let to_string = self.get_property(&receiver, &"toString".into())?;
                    self.stack.push(to_string.clone());
                    if !self.is_callable(&to_string)? {
                        return Err(RuntimeError::TypeError(
                            "toString property is not callable".into(),
                        ));
                    }
                    self.call_native(to_string, receiver, vec![], false)
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ArrayToString => {
                let object = Value::Object(self.coerce_object(&receiver)?);
                self.stack.push(object.clone());
                let join = self.get_property(&object, &"join".into())?;
                if self.is_callable(&join)? {
                    self.call_native(join, object, vec![], false)
                } else {
                    self.native_call(NativeFunction::ObjectToString, object, vec![], false)
                }
            }
            NativeFunction::ArrayConcat => self.array_concat(&receiver, &args),
            NativeFunction::ArrayJoin => self.array_join(&receiver, first),
            NativeFunction::Symbol => Ok(Value::Symbol(JsSymbol::new(
                if matches!(first, Value::Undefined) {
                    None
                } else {
                    Some(self.coerce_string(first)?)
                },
            ))),
            NativeFunction::SymbolFor => {
                let key = self.coerce_string(first)?;
                let symbol = self
                    .symbol_registry
                    .borrow_mut()
                    .entry(key.clone())
                    .or_insert_with(|| JsSymbol::new(Some(key)))
                    .clone();
                Ok(Value::Symbol(symbol))
            }
            NativeFunction::SymbolKeyFor => {
                let Value::Symbol(symbol) = first else {
                    return Err(RuntimeError::TypeError(
                        "Symbol.keyFor requires a Symbol".into(),
                    ));
                };
                Ok(self
                    .symbol_registry
                    .borrow()
                    .iter()
                    .find_map(|(key, candidate)| (candidate == symbol).then(|| key.clone()))
                    .map_or(Value::Undefined, Value::String))
            }
            NativeFunction::Object => {
                // Object(value) normally returns an object argument (or
                // boxes a primitive), but a distinct NewTarget takes the
                // OrdinaryCreateFromConstructor branch first.  This is what
                // makes `class C extends Object {}` and
                // Reflect.construct(Object, values, C) allocate a fresh C
                // instance rather than returning `values[0]`.
                let object_constructor = self.global("Object")?;
                if construct && self.new_target != object_constructor {
                    let prototype = self.constructor_prototype(self.object_prototype)?;
                    return Ok(Value::Object(
                        self.with_roots(|heap| heap.alloc_object(Some(prototype)))?,
                    ));
                }
                if matches!(first, Value::Undefined | Value::Null) {
                    let proto = if construct {
                        self.constructor_prototype(self.object_prototype)?
                    } else {
                        self.object_prototype
                    };
                    return Ok(Value::Object(
                        self.with_roots(|heap| heap.alloc_object(Some(proto)))?,
                    ));
                }
                self.coerce_object(first).map(Value::Object)
            }
            NativeFunction::Iterator => {
                let constructor = self.global("Iterator")?;
                if !construct || self.new_target == constructor {
                    return Err(RuntimeError::TypeError(
                        "Iterator is not directly callable or constructable".into(),
                    ));
                }
                let iterator_prototype = self.base_iterator_prototype()?;
                let prototype = self.constructor_prototype(iterator_prototype)?;
                Ok(Value::Object(
                    self.with_roots(|heap| heap.alloc_object(Some(prototype)))?,
                ))
            }
            NativeFunction::IteratorFrom => {
                if construct {
                    return Err(RuntimeError::TypeError(
                        "Iterator.from is not a constructor".into(),
                    ));
                }
                self.iterator_from(first)
            }
            NativeFunction::IteratorHelper(method) => match method {
                native::IteratorHelperMethod::Concat => self.iterator_concat(&args),
                native::IteratorHelperMethod::Zip => {
                    self.iterator_zip(first, native::argument(&args, 1))
                }
                native::IteratorHelperMethod::ZipKeyed => {
                    self.iterator_zip_keyed(first, native::argument(&args, 1))
                }
                native::IteratorHelperMethod::Chunks => self.iterator_chunks(&receiver, first),
                native::IteratorHelperMethod::Windows => {
                    self.iterator_windows(&receiver, first, native::argument(&args, 1))
                }
                native::IteratorHelperMethod::Map => self.iterator_map(&receiver, first),
                native::IteratorHelperMethod::Filter => self.iterator_filter(&receiver, first),
                native::IteratorHelperMethod::FlatMap => self.iterator_flat_map(&receiver, first),
                native::IteratorHelperMethod::Take => self.iterator_take(&receiver, first),
                native::IteratorHelperMethod::Drop => self.iterator_drop(&receiver, first),
                native::IteratorHelperMethod::Includes => {
                    self.iterator_includes(&receiver, first, native::argument(&args, 1))
                }
                native::IteratorHelperMethod::Join => self.iterator_join(&receiver, first),
            },
            NativeFunction::IteratorToArray => self.iterator_to_array(&receiver),
            NativeFunction::IteratorForEach => self.iterator_for_each(&receiver, first),
            NativeFunction::IteratorEvery => self.iterator_every(&receiver, first),
            NativeFunction::IteratorSome => self.iterator_some(&receiver, first),
            NativeFunction::IteratorFind => self.iterator_find(&receiver, first),
            NativeFunction::IteratorReduce => self.iterator_reduce(&receiver, &args),
            NativeFunction::ObjectMethod(method) => self.object_method(method, &receiver, &args),
            NativeFunction::StringIterator => {
                let string = self.string_receiver(&receiver)?;
                let prototype = self.string_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_string_iterator(string, prototype)
                })?))
            }
            NativeFunction::IteratorNext => {
                let Value::Object(id) = receiver else {
                    return Err(RuntimeError::TypeError(
                        "iterator next requires an iterator".into(),
                    ));
                };
                let Some(value) = self.heap.string_iterator_next(id)? else {
                    return Err(RuntimeError::TypeError(
                        "iterator next requires a String iterator".into(),
                    ));
                };
                let done = value.is_none();
                self.iterator_result(value.map_or(Value::Undefined, Value::String), done)
            }
            NativeFunction::IteratorWrapperNext => self.iterator_wrapper_next(&receiver),
            NativeFunction::IteratorWrapperReturn => self.iterator_wrapper_return(&receiver),
            NativeFunction::IteratorHelperNext => self.iterator_helper_next(&receiver),
            NativeFunction::IteratorHelperReturn => self.iterator_helper_return(&receiver),
            NativeFunction::IteratorDispose => self.iterator_dispose(&receiver),
            NativeFunction::IteratorConstructorGetter => self.global("Iterator"),
            NativeFunction::IteratorConstructorSetter => {
                self.iterator_constructor_setter(&receiver, first)
            }
            NativeFunction::IteratorToStringTagGetter => Ok(Value::String("Iterator".into())),
            NativeFunction::IteratorToStringTagSetter => {
                self.iterator_to_string_tag_setter(&receiver, first)
            }
            NativeFunction::IteratorSelf | NativeFunction::AsyncIteratorSelf => Ok(receiver),
            NativeFunction::Pattern(method) => self.string_pattern(method, &receiver, &args),
            NativeFunction::String => {
                let string = if args.is_empty() {
                    JsString::default()
                } else {
                    self.string_constructor_argument(native::argument(&args, 0), construct)?
                };
                self.check_string(&Value::String(string.clone()))?;
                if construct {
                    let (_, prototype) = self.string_intrinsics()?;
                    let prototype = self.constructor_prototype(prototype)?;
                    Ok(Value::Object(self.with_roots(|heap| {
                        heap.alloc_string(string, Some(prototype))
                    })?))
                } else {
                    Ok(Value::String(string))
                }
            }
            NativeFunction::FromCharCode | NativeFunction::FromCodePoint => {
                let mut result = JsString::default();
                for arg in &args {
                    let number = Value::Number(self.coerce_number(arg)?);
                    let Value::String(part) = native::from_codes(
                        &[number],
                        function == NativeFunction::FromCodePoint,
                        self.config.max_string_bytes,
                    )?
                    else {
                        unreachable!()
                    };
                    native::append(&mut result, &part, self.config.max_string_bytes)?;
                }
                Ok(Value::String(result))
            }
            NativeFunction::Raw => self.string_raw(&args),
            NativeFunction::Split => self.string_split(&receiver, &args),
            NativeFunction::Replace | NativeFunction::ReplaceAll => {
                self.string_replace(&receiver, &args, function == NativeFunction::ReplaceAll)
            }
            NativeFunction::StringMethod(method) => {
                self.dispatch_string_method(method, &receiver, &args)
            }
            NativeFunction::Call => self.call_native(
                receiver,
                first.clone(),
                args.iter().skip(1).cloned().collect(),
                false,
            ),
        }
    }
}
