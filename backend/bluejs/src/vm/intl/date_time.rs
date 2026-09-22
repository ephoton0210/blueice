// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn date_time_width(
        &mut self,
        options: &Value,
        name: &str,
        allowed: &[&str],
    ) -> Result<Option<blueice_ecma402::DateTimeWidth>, RuntimeError> {
        self.string_option(options, name, allowed)?
            .map_or(Ok(None), |value| {
                Ok(Some(match value.as_str() {
                    "numeric" => blueice_ecma402::DateTimeWidth::Numeric,
                    "2-digit" => blueice_ecma402::DateTimeWidth::TwoDigit,
                    "short" => blueice_ecma402::DateTimeWidth::Short,
                    "long" => blueice_ecma402::DateTimeWidth::Long,
                    "narrow" => blueice_ecma402::DateTimeWidth::Narrow,
                    _ => unreachable!("string_option validates date-time field widths"),
                }))
            })
    }

    pub(in super::super) fn date_time_style(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<blueice_ecma402::DateTimeStyle>, RuntimeError> {
        self.string_option(options, name, &["full", "long", "medium", "short"])?
            .map_or(Ok(None), |value| {
                Ok(Some(match value.as_str() {
                    "full" => blueice_ecma402::DateTimeStyle::Full,
                    "long" => blueice_ecma402::DateTimeStyle::Long,
                    "medium" => blueice_ecma402::DateTimeStyle::Medium,
                    "short" => blueice_ecma402::DateTimeStyle::Short,
                    _ => unreachable!("string_option validates date-time styles"),
                }))
            })
    }

    pub(in super::super) fn date_time_fractional_second_digits(
        &mut self,
        options: &Value,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &"fractionalSecondDigits".into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let value = self.coerce_number(&value)?;
        // GetNumberOption first validates the numeric value against its
        // inclusive bounds and only then applies floor. Thus 2.9 resolves to
        // 2, whereas 3.000001 remains out of range.
        if !value.is_finite() || !(1.0..=3.0).contains(&value) {
            return Err(RuntimeError::RangeError(
                "invalid fractionalSecondDigits option".into(),
            ));
        }
        Ok(Some(value.floor() as u8))
    }

    pub(in super::super) fn date_time_format_options(
        &mut self,
        value: &Value,
    ) -> Result<blueice_ecma402::DateTimeFormatOptions, RuntimeError> {
        let options = self.intl_options(value)?;
        let locale_matcher = self.locale_matcher(&options)?;
        let calendar = self.string_option(&options, "calendar", &[])?;
        let numbering_system = self.string_option(&options, "numberingSystem", &[])?;
        if let Some(value) = &numbering_system {
            if !(3..=8).contains(&value.len())
                || !value
                    .bytes()
                    .all(|character| character.is_ascii_alphanumeric())
            {
                return Err(RuntimeError::RangeError(
                    "invalid numberingSystem option".into(),
                ));
            }
        }
        let hour12 = self.get_property(&options, &"hour12".into())?;
        let hour12 = if hour12 == Value::Undefined {
            None
        } else {
            Some(self.to_boolean(&hour12)?)
        };
        let hour_cycle =
            self.string_option(&options, "hourCycle", &["h11", "h12", "h23", "h24"])?;
        let time_zone = self.string_option(&options, "timeZone", &[])?;
        let weekday = self.date_time_width(&options, "weekday", &["short", "long", "narrow"])?;
        let era = self.date_time_width(&options, "era", &["short", "long", "narrow"])?;
        let year = self.date_time_width(&options, "year", &["numeric", "2-digit"])?;
        let month = self.date_time_width(
            &options,
            "month",
            &["numeric", "2-digit", "short", "long", "narrow"],
        )?;
        let day = self.date_time_width(&options, "day", &["numeric", "2-digit"])?;
        let day_period =
            self.date_time_width(&options, "dayPeriod", &["narrow", "short", "long"])?;
        let hour = self.date_time_width(&options, "hour", &["numeric", "2-digit"])?;
        let minute = self.date_time_width(&options, "minute", &["numeric", "2-digit"])?;
        let second = self.date_time_width(&options, "second", &["numeric", "2-digit"])?;
        let fractional_second_digits = self.date_time_fractional_second_digits(&options)?;
        let time_zone_name = self.string_option(
            &options,
            "timeZoneName",
            &[
                "long",
                "short",
                "shortOffset",
                "longOffset",
                "shortGeneric",
                "longGeneric",
            ],
        )?;
        let format_matcher =
            match self.string_option(&options, "formatMatcher", &["basic", "best fit"])? {
                Some(value) if value == "basic" => blueice_ecma402::DateTimeFormatMatcher::Basic,
                Some(value) if value == "best fit" => {
                    blueice_ecma402::DateTimeFormatMatcher::BestFit
                }
                None => blueice_ecma402::DateTimeFormatMatcher::BestFit,
                Some(_) => unreachable!("string_option validates formatMatcher values"),
            };
        let date_style = self.date_time_style(&options, "dateStyle")?;
        let time_style = self.date_time_style(&options, "timeStyle")?;
        let has_components = weekday.is_some()
            || era.is_some()
            || year.is_some()
            || month.is_some()
            || day.is_some()
            || day_period.is_some()
            || hour.is_some()
            || minute.is_some()
            || second.is_some()
            || fractional_second_digits.is_some()
            || time_zone_name.is_some();
        if (date_style.is_some() || time_style.is_some()) && has_components {
            return Err(RuntimeError::TypeError(
                "dateStyle and timeStyle cannot be used with date-time component options".into(),
            ));
        }
        Ok(blueice_ecma402::DateTimeFormatOptions {
            locale_matcher,
            format_matcher,
            calendar,
            numbering_system,
            hour_cycle,
            hour12,
            time_zone,
            weekday,
            era,
            year,
            month,
            day,
            day_period,
            hour,
            minute,
            second,
            fractional_second_digits,
            time_zone_name,
            date_style,
            time_style,
        })
    }

    pub(in super::super) fn resolve_date_time_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::DateTimeFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.date_time_format_options(options)?;
        blueice_ecma402::DateTimeFormat::try_new(&locales, options)
            .map(Rc::new)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    /// Shared `Date.prototype.toLocale*` bridge. The legacy Date methods do
    /// not use the fixed English Date string helpers: they run the same
    /// DateTimeFormat resolution as an explicit formatter after
    /// `ToDateTimeOptions` supplies only the defaults required by that
    /// particular method.
    pub(in super::super) fn date_to_locale_string(
        &mut self,
        time: f64,
        args: &[Value],
        default_date: bool,
        default_time: bool,
    ) -> Result<Value, RuntimeError> {
        if !time.is_finite() {
            return Ok(Value::String("Invalid Date".into()));
        }
        let mut options = self.date_time_format_options(native::argument(args, 1))?;
        let has_date = options.weekday.is_some()
            || options.year.is_some()
            || options.month.is_some()
            || options.day.is_some();
        let has_time = options.day_period.is_some()
            || options.hour.is_some()
            || options.minute.is_some()
            || options.second.is_some()
            || options.fractional_second_digits.is_some();
        let has_style = options.date_style.is_some() || options.time_style.is_some();

        // ToDateTimeOptions has three distinct defaulting modes. `any` (the
        // all-locales method) defaults only when neither group was supplied;
        // the date/time methods independently fill their required group while
        // retaining any caller-supplied fields from the other group.
        let need_date = default_date
            && !has_style
            && if default_time {
                !has_date && !has_time
            } else {
                !has_date
            };
        let need_time = default_time
            && !has_style
            && if default_date {
                !has_date && !has_time
            } else {
                !has_time
            };
        if need_date {
            options.year = Some(blueice_ecma402::DateTimeWidth::Numeric);
            options.month = Some(blueice_ecma402::DateTimeWidth::Numeric);
            options.day = Some(blueice_ecma402::DateTimeWidth::Numeric);
        }
        if need_time {
            options.hour = Some(blueice_ecma402::DateTimeWidth::Numeric);
            options.minute = Some(blueice_ecma402::DateTimeWidth::Numeric);
            options.second = Some(blueice_ecma402::DateTimeWidth::Numeric);
        }
        // Date.prototype.toLocale* runs ToDateTimeOptions before it delegates
        // to CreateDateTimeFormat, so an abrupt option getter wins over a
        // later locale coercion just as it does in the specification.
        let locales = self.canonical_locales(native::argument(args, 0))?;
        blueice_ecma402::DateTimeFormat::try_new(&locales, options)
            .and_then(|format| format.format(time))
            .map(|formatted| Value::String(formatted.into()))
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn date_time_format_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let locales = blueice_ecma402::supported_locales(
            blueice_ecma402::IntlService::DateTimeFormat,
            &locales,
            self.locale_matcher(&options)?,
        );
        self.array_from(
            locales
                .into_iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    pub(in super::super) fn create_date_time_format(
        &mut self,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        self.intl_global()?;
        let constructor = self.globals["%Intl.DateTimeFormat%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.DateTimeFormat.prototype is an object");
        let legacy_receiver =
            (!construct && self.intl_legacy_receiver(receiver, default)?).then(|| receiver.clone());
        let prototype = if construct {
            self.constructor_prototype(default)?
        } else {
            default
        };
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_date_time_format(native::argument(args, 0), native::argument(args, 1))?;
        let date_time_format =
            self.with_roots(|heap| heap.alloc_date_time_format(data, prototype))?;
        let Some(legacy_receiver) = legacy_receiver else {
            return Ok(Value::Object(date_time_format));
        };

        // ECMA-402's normative-optional ChainDateTimeFormat mode preserves
        // the eligible call receiver, while a real DateTimeFormat object is
        // kept behind a per-realm non-enumerable, non-writable and
        // non-configurable Symbol property. Use the generic internal method
        // here so an eligible Proxy observes [[DefineOwnProperty]].
        let fallback_symbol = self.intl_legacy_fallback_symbol();
        let legacy_id = legacy_receiver
            .object_id()
            .expect("legacy DateTimeFormat receiver is an object");
        self.stack.push(Value::Object(date_time_format));
        if !self.object_define_own_property(
            legacy_id,
            fallback_symbol.into(),
            PropertyDescriptor::data(Value::Object(date_time_format), false, false, false),
        )? {
            return Err(RuntimeError::TypeError(
                "cannot define IntlLegacyConstructedSymbol property".into(),
            ));
        }
        Ok(legacy_receiver)
    }

    /// Returns whether the service prototype occurs strictly in `receiver`'s
    /// prototype chain. This is the receiver predicate for the
    /// normative-optional legacy constructor behavior.
    pub(in super::super) fn intl_legacy_receiver(
        &mut self,
        receiver: &Value,
        prototype: ObjectId,
    ) -> Result<bool, RuntimeError> {
        let Some(mut object) = receiver.object_id() else {
            return Ok(false);
        };
        let base = self.stack.len();
        self.stack
            .extend([receiver.clone(), Value::Object(prototype)]);
        let result = (|| {
            loop {
                // Proxy [[GetPrototypeOf]] can execute arbitrary JavaScript,
                // so retain the current link across that operation.
                self.stack[base] = Value::Object(object);
                let Some(parent) = self.object_get_prototype(object)? else {
                    return Ok(false);
                };
                if parent == prototype {
                    return Ok(true);
                }
                object = parent;
            }
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn intl_legacy_fallback_symbol(&mut self) -> JsSymbol {
        self.intl_legacy_constructed_symbol
            .get_or_insert_with(|| JsSymbol::new(Some("IntlLegacyConstructedSymbol".into())))
            .clone()
    }

    pub(in super::super) fn date_time_format_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::DateTimeFormat>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.date_time_format(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.DateTimeFormat".into(),
        ))
    }

    /// Implements `UnwrapDateTimeFormat` for the only legacy-facing methods
    /// that ECMA-402 permits to unwrap a ChainDateTimeFormat receiver. Other
    /// DateTimeFormat methods deliberately continue to call
    /// `date_time_format_data` and require a directly branded receiver.
    pub(in super::super) fn unwrap_date_time_format(
        &mut self,
        value: &Value,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(id) = value.object_id() {
            if self.heap.date_time_format(id)?.is_some() {
                return Ok(id);
            }
        } else {
            return Err(RuntimeError::TypeError(
                "receiver is not an Intl.DateTimeFormat".into(),
            ));
        }

        let fallback_symbol = self.intl_legacy_constructed_symbol.clone().ok_or_else(|| {
            RuntimeError::TypeError("receiver is not an Intl.DateTimeFormat".into())
        })?;
        // `Get` is intentional. In particular, a Proxy around a chained
        // receiver must observe this symbol lookup before the hidden object
        // is brand-checked.
        let fallback_key = PropertyName::from(fallback_symbol);
        let fallback = self.get_property(value, &fallback_key)?;
        let Some(id) = fallback.object_id() else {
            return Err(RuntimeError::TypeError(
                "receiver is not an Intl.DateTimeFormat".into(),
            ));
        };
        self.heap
            .date_time_format(id)?
            .is_some()
            .then_some(id)
            .ok_or_else(|| RuntimeError::TypeError("receiver is not an Intl.DateTimeFormat".into()))
    }

    pub(in super::super) fn date_time_format_format_getter(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.unwrap_date_time_format(receiver)?;
        if let Some(function) = self.heap.date_time_format_format(id) {
            return Ok(Value::Object(function));
        }
        let constructor = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(constructor)?.unwrap();
        let target = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::DateTimeFormatFormat, "", prototype)
        })?;
        let function = self.with_roots(|heap| {
            heap.alloc_bound_function(
                crate::heap::BoundFunction {
                    target,
                    // The getter may have received a legacy chained object.
                    // Bind the actual branded formatter selected by
                    // UnwrapDateTimeFormat, not that outer receiver.
                    this: Value::Object(id),
                    args: vec![],
                    constructible: false,
                },
                Some(prototype),
            )
        })?;
        self.stack.push(Value::Object(function));
        self.define_data(function, "length", Value::Number(1.0), false, false, true)?;
        self.define_data(
            function,
            "name",
            Value::String("".into()),
            false,
            false,
            true,
        )?;
        self.heap.set_date_time_format_format(id, function);
        Ok(Value::Object(function))
    }

    pub(in super::super) fn date_time_value(&mut self, value: &Value) -> Result<f64, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(Self::current_time());
        }
        let value = self.coerce_number(value)?;
        if !value.is_finite() {
            return Err(RuntimeError::RangeError("invalid time value".into()));
        }
        Ok(value)
    }

    pub(in super::super) fn date_time_format_format(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.date_time_format_data(receiver)?;
        let parts = self.date_time_format_parts(&data, value)?;
        Ok(Value::String(
            parts
                .into_iter()
                .map(|part| part.value)
                .collect::<String>()
                .into(),
        ))
    }

    pub(in super::super) fn date_time_parts_to_value(
        &mut self,
        parts: Vec<blueice_ecma402::DateTimePart>,
        source: Option<&str>,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for part in parts {
                let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(object));
                self.define_data(
                    object,
                    "type",
                    Value::String(part.kind.into()),
                    true,
                    true,
                    true,
                )?;
                self.define_data(
                    object,
                    "value",
                    Value::String(part.value.into()),
                    true,
                    true,
                    true,
                )?;
                if let Some(source) = source {
                    self.define_data(
                        object,
                        "source",
                        Value::String(source.into()),
                        true,
                        true,
                        true,
                    )?;
                }
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn date_time_range_parts_to_value(
        &mut self,
        parts: Vec<blueice_ecma402::DateTimeRangePart>,
    ) -> Result<Value, RuntimeError> {
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for part in parts {
                let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(object));
                self.define_data(
                    object,
                    "type",
                    Value::String(part.kind.into()),
                    true,
                    true,
                    true,
                )?;
                self.define_data(
                    object,
                    "value",
                    Value::String(part.value.into()),
                    true,
                    true,
                    true,
                )?;
                let source = match part.source {
                    blueice_ecma402::DateTimeRangePartSource::Shared => "shared",
                    blueice_ecma402::DateTimeRangePartSource::StartRange => "startRange",
                    blueice_ecma402::DateTimeRangePartSource::EndRange => "endRange",
                };
                self.define_data(
                    object,
                    "source",
                    Value::String(source.into()),
                    true,
                    true,
                    true,
                )?;
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn date_time_format_format_to_parts(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.date_time_format_data(receiver)?;
        let parts = self.date_time_format_parts(&data, value)?;
        self.date_time_parts_to_value(parts, None)
    }

    pub(in super::super) fn date_time_format_parts(
        &mut self,
        data: &blueice_ecma402::DateTimeFormat,
        value: &Value,
    ) -> Result<Vec<blueice_ecma402::DateTimePart>, RuntimeError> {
        let value = self.date_time_format_value(value, true)?;
        let input = self.date_time_format_input(data, value)?;
        data.format_input_to_parts(input)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn date_time_format_value(
        &mut self,
        value: &Value,
        default_to_now: bool,
    ) -> Result<DateTimeFormatValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                return Ok(DateTimeFormatValue::Temporal(temporal));
            }
        }
        let epoch_milliseconds = if default_to_now {
            self.date_time_value(value)?
        } else {
            self.coerce_number(value)?
        };
        Ok(DateTimeFormatValue::Number(epoch_milliseconds))
    }

    pub(in super::super) fn date_time_format_input(
        &self,
        data: &blueice_ecma402::DateTimeFormat,
        value: DateTimeFormatValue,
    ) -> Result<blueice_ecma402::DateTimeFormatInput, RuntimeError> {
        match value {
            DateTimeFormatValue::Number(epoch_milliseconds) => Ok(
                blueice_ecma402::DateTimeFormatInput::EpochMilliseconds(epoch_milliseconds),
            ),
            DateTimeFormatValue::Temporal(temporal) => {
                if temporal.kind == TemporalKind::ZonedDateTime {
                    return Err(RuntimeError::TypeError(
                        "Intl.DateTimeFormat does not support Temporal.ZonedDateTime".into(),
                    ));
                }
                if temporal.kind == TemporalKind::Duration {
                    return Err(RuntimeError::TypeError(
                        "Intl.DateTimeFormat does not support Temporal.Duration".into(),
                    ));
                }
                Self::temporal_check_format_calendar(data, &temporal)?;
                let options = self.temporal_format_options(data, temporal.kind)?;
                self.temporal_date_time_format_input(temporal, options)
            }
        }
    }

    /// ECMA-402's `HandleDateTimeTemporalDate`/`...YearMonth`/`...MonthDay`
    /// calendar rule: a plain Temporal value can only be formatted by a
    /// formatter using its own calendar. `PlainDate` and `PlainDateTime` in
    /// the ISO calendar are the exception (their fields read the same in any
    /// calendar), as is a `ZonedDateTime` (which `toLocaleString` formats
    /// directly); `PlainYearMonth` and `PlainMonthDay` have none, because their
    /// ISO reference day/year would be misleading in another calendar. An
    /// `Instant` and a `PlainTime` carry no calendar.
    pub(in super::super) fn temporal_check_format_calendar(
        data: &blueice_ecma402::DateTimeFormat,
        temporal: &TemporalValue,
    ) -> Result<(), RuntimeError> {
        let iso_allowed = match temporal.kind {
            TemporalKind::PlainDate | TemporalKind::PlainDateTime | TemporalKind::ZonedDateTime => {
                true
            }
            TemporalKind::PlainYearMonth | TemporalKind::PlainMonthDay => false,
            _ => return Ok(()),
        };
        if (iso_allowed && temporal.calendar == "iso8601") || temporal.calendar == data.calendar() {
            return Ok(());
        }
        Err(RuntimeError::RangeError(format!(
            "the {} calendar of a Temporal.{} does not match the formatter's {} calendar",
            temporal.calendar,
            temporal.kind.name(),
            data.calendar()
        )))
    }

    pub(in super::super) fn temporal_date_time_format_input(
        &self,
        temporal: TemporalValue,
        options: blueice_ecma402::DateTimeFormatOptions,
    ) -> Result<blueice_ecma402::DateTimeFormatInput, RuntimeError> {
        match temporal.kind {
            TemporalKind::Duration => Err(RuntimeError::TypeError(
                "Intl.DateTimeFormat does not support Temporal.Duration".into(),
            )),
            TemporalKind::ZonedDateTime => Err(RuntimeError::TypeError(
                "Intl.DateTimeFormat does not support Temporal.ZonedDateTime".into(),
            )),
            TemporalKind::Instant => {
                let milliseconds = (&temporal.epoch_nanoseconds / 1_000_000u32)
                    .to_f64()
                    .ok_or_else(|| RuntimeError::RangeError("invalid Temporal instant".into()))?;
                Ok(blueice_ecma402::DateTimeFormatInput::TemporalInstant {
                    epoch_milliseconds: milliseconds,
                    options,
                })
            }
            _ => Ok(blueice_ecma402::DateTimeFormatInput::TemporalPlain {
                local_epoch_milliseconds: temporal.plain_epoch_milliseconds(),
                options,
            }),
        }
    }

    pub(in super::super) fn date_time_range_values(
        &mut self,
        start: &Value,
        end: &Value,
    ) -> Result<(DateTimeFormatValue, DateTimeFormatValue), RuntimeError> {
        // Unlike `format` and `formatToParts`, the range methods require both
        // operands. Check that before ToNumber so a missing endpoint takes
        // precedence over observable conversion of the other argument.
        if *start == Value::Undefined || *end == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "date-time range endpoints must not be undefined".into(),
            ));
        }
        // Convert both arguments before checking their kinds. In particular,
        // an ordinary object's `valueOf` remains observable when the other
        // argument is a Temporal object, as required by ToDateTimeFormattable.
        Ok((
            self.date_time_format_value(start, false)?,
            self.date_time_format_value(end, false)?,
        ))
    }

    pub(in super::super) fn temporal_format_options(
        &self,
        data: &blueice_ecma402::DateTimeFormat,
        kind: TemporalKind,
    ) -> Result<blueice_ecma402::DateTimeFormatOptions, RuntimeError> {
        if kind == TemporalKind::Duration {
            return Err(RuntimeError::TypeError(
                "Intl.DateTimeFormat does not support Temporal.Duration".into(),
            ));
        }
        let original = data.options();
        let has_date = temporal_has_date_components(original);
        let has_time = temporal_has_time_components(original);
        // `era` is an additive display field. It does not suppress the
        // type-specific default components selected by DateTimeFormat.
        let only_default_components = !(original.date_style.is_some()
            || original.time_style.is_some()
            || has_time
            || original.weekday.is_some()
            || original.year.is_some()
            || original.month.is_some()
            || original.day.is_some());
        let mut options = original.clone();
        if kind == TemporalKind::Instant {
            if only_default_components {
                temporal_default_components(&mut options, kind);
            }
            return Ok(options);
        }
        // Plain Temporal values denote local calendar fields, not instants.
        // UTC carries those fields through ICU4X without applying the
        // formatter's requested IANA transition rules.
        options.time_zone = Some("UTC".into());
        options.time_zone_name = None;
        match kind {
            TemporalKind::PlainDate => clear_temporal_time_components(&mut options),
            TemporalKind::PlainDateTime => {}
            TemporalKind::PlainMonthDay => {
                clear_temporal_time_components(&mut options);
                options.weekday = None;
                options.era = None;
                options.year = None;
                if let Some(style) = original.date_style {
                    apply_temporal_partial_date_style(&mut options, style, false);
                }
            }
            TemporalKind::PlainTime => clear_temporal_date_components(&mut options),
            TemporalKind::PlainYearMonth => {
                clear_temporal_time_components(&mut options);
                options.weekday = None;
                options.day = None;
                if let Some(style) = original.date_style {
                    apply_temporal_partial_date_style(&mut options, style, true);
                }
            }
            TemporalKind::Instant | TemporalKind::ZonedDateTime | TemporalKind::Duration => {
                unreachable!()
            }
        }
        // `timeStyle: long/full` normally supplies a time-zone name. Plain
        // values specifically suppress it, so retain the time fields with an
        // otherwise equivalent non-zone style.
        if matches!(
            options.time_style,
            Some(blueice_ecma402::DateTimeStyle::Full | blueice_ecma402::DateTimeStyle::Long)
        ) {
            options.time_style = Some(blueice_ecma402::DateTimeStyle::Medium);
        }
        if only_default_components {
            temporal_default_components(&mut options, kind);
        }
        let visible = only_default_components
            || options.date_style.is_some()
            || options.time_style.is_some()
            || temporal_has_date_components(&options)
            || temporal_has_time_components(&options);
        if !visible {
            return Err(RuntimeError::TypeError(
                "DateTimeFormat options do not overlap the Temporal value".into(),
            ));
        }
        // If a date/time style was present but it did not apply to this
        // Temporal kind, the pruning above left no components. This is the
        // same no-overlap TypeError, including dateStyle with PlainTime.
        // This is a *value-side* check (it runs for every formatted value,
        // including a plain `Intl.DateTimeFormat.prototype.format` call, not
        // only `Temporal.PlainTime.prototype.toLocaleString`) and is
        // deliberately narrower than `toLocaleString`'s own required=TIME
        // check in `temporal_plain_time_to_locale_string`: a bare
        // `{ dateStyle }` (no timeStyle, no time fields) has no overlap with
        // a PlainTime under either rule, but `{ dateStyle, timeStyle }`
        // together format the value fine here — dateStyle is simply ignored
        // — per `intl402/DateTimeFormat/prototype/format/
        // temporal-plaintime-formatting-datetime-style.js`, which is *not*
        // going through `toLocaleString`.
        if (kind == TemporalKind::PlainTime
            && original.date_style.is_some()
            && original.time_style.is_none()
            && !has_time)
            || (kind == TemporalKind::PlainDate
                && original.time_style.is_some()
                && original.date_style.is_none()
                && !has_date)
        {
            return Err(RuntimeError::TypeError(
                "DateTimeFormat options do not overlap the Temporal value".into(),
            ));
        }
        Ok(options)
    }

    pub(in super::super) fn temporal_range_parts(
        &self,
        data: &blueice_ecma402::DateTimeFormat,
        start: TemporalValue,
        end: TemporalValue,
    ) -> Result<Vec<blueice_ecma402::DateTimeRangePart>, RuntimeError> {
        if start.kind != end.kind {
            return Err(RuntimeError::TypeError(
                "date-time range endpoints have different Temporal types".into(),
            ));
        }
        if start.calendar != end.calendar {
            return Err(RuntimeError::RangeError(
                "date-time range endpoints have different calendars".into(),
            ));
        }
        if start.kind == TemporalKind::ZonedDateTime {
            return Err(RuntimeError::TypeError(
                "Intl.DateTimeFormat does not support Temporal.ZonedDateTime".into(),
            ));
        }
        // Both endpoints share a Temporal kind and calendar above, so one
        // resolved option record -- and one calendar comparison -- must govern
        // direct and range formatting.
        Self::temporal_check_format_calendar(data, &start)?;
        let options = self.temporal_format_options(data, start.kind)?;
        let start = self.temporal_date_time_format_input(start, options.clone())?;
        let end = self.temporal_date_time_format_input(end, options)?;
        data.format_range_inputs_to_parts(start, end)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn date_time_range_parts(
        &self,
        data: &blueice_ecma402::DateTimeFormat,
        start: DateTimeFormatValue,
        end: DateTimeFormatValue,
    ) -> Result<Vec<blueice_ecma402::DateTimeRangePart>, RuntimeError> {
        match (start, end) {
            (DateTimeFormatValue::Number(start), DateTimeFormatValue::Number(end)) => data
                .format_range_inputs_to_parts(
                    blueice_ecma402::DateTimeFormatInput::EpochMilliseconds(start),
                    blueice_ecma402::DateTimeFormatInput::EpochMilliseconds(end),
                )
                .map_err(|error| RuntimeError::RangeError(error.to_string())),
            (DateTimeFormatValue::Temporal(start), DateTimeFormatValue::Temporal(end)) => {
                self.temporal_range_parts(data, start, end)
            }
            _ => Err(RuntimeError::TypeError(
                "date-time range endpoints have different kinds".into(),
            )),
        }
    }

    pub(in super::super) fn date_time_format_format_range(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.date_time_format_data(receiver)?;
        let (start, end) = self.date_time_range_values(start, end)?;
        self.date_time_range_parts(&data, start, end).map(|parts| {
            Value::String(
                parts
                    .into_iter()
                    .map(|part| part.value)
                    .collect::<String>()
                    .into(),
            )
        })
    }

    pub(in super::super) fn date_time_format_format_range_to_parts(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.date_time_format_data(receiver)?;
        let (start, end) = self.date_time_range_values(start, end)?;
        let parts = self.date_time_range_parts(&data, start, end)?;
        self.date_time_range_parts_to_value(parts)
    }

    pub(in super::super) fn date_time_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.unwrap_date_time_format(receiver)?;
        let data = self
            .heap
            .date_time_format(id)?
            .expect("UnwrapDateTimeFormat returns a branded object");
        let options = data.options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (name, value) in [
            ("locale", Value::String(data.locale().into())),
            ("calendar", Value::String(data.calendar().into())),
            (
                "numberingSystem",
                Value::String(data.numbering_system().into()),
            ),
            ("timeZone", Value::String(data.time_zone().into())),
        ] {
            self.define_data(result, name, value, true, true, true)?;
        }
        let time_requested = options.time_style.is_some()
            || options.hour.is_some()
            || options.minute.is_some()
            || options.second.is_some()
            || options.fractional_second_digits.is_some();
        // InitializeDateTimeFormat supplies the numeric date triple only
        // when *no* date or time component was requested. Do not turn a
        // requested year/month, month/day, or weekday skeleton into YMD in
        // resolvedOptions merely because it has no time fields.
        let default_date = !time_requested
            && options.date_style.is_none()
            && options.weekday.is_none()
            && options.year.is_none()
            && options.month.is_none()
            && options.day.is_none();
        let default_date_width = default_date.then_some(blueice_ecma402::DateTimeWidth::Numeric);
        if time_requested {
            self.define_data(
                result,
                "hourCycle",
                Value::String(data.hour_cycle().into()),
                true,
                true,
                true,
            )?;
            self.define_data(
                result,
                "hour12",
                Value::Bool(matches!(data.hour_cycle(), "h11" | "h12")),
                true,
                true,
                true,
            )?;
        }
        for (name, width) in [
            ("weekday", options.weekday),
            ("era", options.era),
            ("year", options.year.or(default_date_width)),
            ("month", options.month.or(default_date_width)),
            ("day", options.day.or(default_date_width)),
            ("dayPeriod", options.day_period),
            ("hour", options.hour),
            ("minute", options.minute),
            ("second", options.second),
        ] {
            if let Some(width) = width {
                let width = match width {
                    blueice_ecma402::DateTimeWidth::Numeric => "numeric",
                    blueice_ecma402::DateTimeWidth::TwoDigit => "2-digit",
                    blueice_ecma402::DateTimeWidth::Short => "short",
                    blueice_ecma402::DateTimeWidth::Long => "long",
                    blueice_ecma402::DateTimeWidth::Narrow => "narrow",
                };
                self.define_data(result, name, Value::String(width.into()), true, true, true)?;
            }
        }
        if let Some(digits) = options.fractional_second_digits {
            self.define_data(
                result,
                "fractionalSecondDigits",
                Value::Number(digits.into()),
                true,
                true,
                true,
            )?;
        }
        if let Some(name) = &options.time_zone_name {
            self.define_data(
                result,
                "timeZoneName",
                Value::String(name.clone().into()),
                true,
                true,
                true,
            )?;
        }
        if let Some(style) = options.date_style {
            self.define_data(
                result,
                "dateStyle",
                Value::String(
                    match style {
                        blueice_ecma402::DateTimeStyle::Full => "full",
                        blueice_ecma402::DateTimeStyle::Long => "long",
                        blueice_ecma402::DateTimeStyle::Medium => "medium",
                        blueice_ecma402::DateTimeStyle::Short => "short",
                    }
                    .into(),
                ),
                true,
                true,
                true,
            )?;
        }
        if let Some(style) = options.time_style {
            self.define_data(
                result,
                "timeStyle",
                Value::String(
                    match style {
                        blueice_ecma402::DateTimeStyle::Full => "full",
                        blueice_ecma402::DateTimeStyle::Long => "long",
                        blueice_ecma402::DateTimeStyle::Medium => "medium",
                        blueice_ecma402::DateTimeStyle::Short => "short",
                    }
                    .into(),
                ),
                true,
                true,
                true,
            )?;
        }
        Ok(Value::Object(result))
    }
}
