// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn list_type(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::ListType, RuntimeError> {
        let value = self.get_property(options, &"type".into())?;
        if value == Value::Undefined {
            return Ok(blueice_ecma402::ListType::Conjunction);
        }
        match self
            .coerce_string(&value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid type option".into()))?
            .as_str()
        {
            "conjunction" => Ok(blueice_ecma402::ListType::Conjunction),
            "disjunction" => Ok(blueice_ecma402::ListType::Disjunction),
            "unit" => Ok(blueice_ecma402::ListType::Unit),
            _ => Err(RuntimeError::RangeError("invalid type option".into())),
        }
    }

    pub(in super::super) fn list_style(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::ListStyle, RuntimeError> {
        let value = self.get_property(options, &"style".into())?;
        if value == Value::Undefined {
            return Ok(blueice_ecma402::ListStyle::Wide);
        }
        match self
            .coerce_string(&value)?
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid style option".into()))?
            .as_str()
        {
            "long" => Ok(blueice_ecma402::ListStyle::Wide),
            "short" => Ok(blueice_ecma402::ListStyle::Short),
            "narrow" => Ok(blueice_ecma402::ListStyle::Narrow),
            _ => Err(RuntimeError::RangeError("invalid style option".into())),
        }
    }

    pub(in super::super) fn resolve_list_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::ListFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_constructor_options(options)?;
        let options = blueice_ecma402::ListFormatOptions {
            locale_matcher: self.locale_matcher(&options)?,
            list_type: self.list_type(&options)?,
            style: self.list_style(&options)?,
        };
        blueice_ecma402::ListFormat::try_new(&locales, options)
            .map(Rc::new)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn list_format_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_list_format_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    pub(in super::super) fn create_list_format(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.ListFormat must be called with new".into(),
            ));
        }
        self.intl_global()?;
        let constructor = self.globals["%Intl.ListFormat%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.ListFormat.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_list_format(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_list_format(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn list_format_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::ListFormat>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.list_format(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.ListFormat".into(),
        ))
    }

    pub(in super::super) fn list_format_values(
        &mut self,
        value: &Value,
    ) -> Result<Vec<JsString>, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(Vec::new());
        }
        let base = self.stack.len();
        self.stack.push(value.clone());
        let result = (|| {
            let record = self.get_iterator(value)?;
            self.stack.push(record.clone());
            let outcome = (|| {
                let mut values = Vec::new();
                while let Some(value) = self.iterator_step(&record, true)? {
                    let Value::String(value) = value else {
                        return Err(RuntimeError::TypeError(
                            "Intl.ListFormat list elements must be strings".into(),
                        ));
                    };
                    values.push(value);
                }
                Ok(values)
            })();
            if outcome.is_err() {
                // StringListFromIterable closes the live iterator, preserving
                // the original abrupt completion if close itself fails.
                let _ = self.iterator_close(&record);
            }
            outcome
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn list_format_parts(
        &mut self,
        receiver: &Value,
        list: &Value,
    ) -> Result<Vec<(blueice_ecma402::ListPartKind, JsString)>, RuntimeError> {
        let data = self.list_format_data(receiver)?;
        let values = self.list_format_values(list)?;
        // ICU4X operates on Unicode scalar strings, while ECMAScript strings
        // retain lone UTF-16 surrogates. Use a scalar projection only for
        // CLDR pattern selection and put the original values back into every
        // element part, preserving the observable ECMAScript code units.
        let projected = values
            .iter()
            .map(|value| {
                char::decode_utf16(value.as_code_units().iter().copied())
                    .map(|scalar| scalar.unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let parts = data
            .format_to_parts(projected.iter())
            .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        let mut next_element = 0usize;
        parts
            .into_iter()
            .map(|part| match part.kind {
                blueice_ecma402::ListPartKind::Element => {
                    let value = values.get(next_element).cloned().ok_or_else(|| {
                        RuntimeError::RangeError("invalid ListFormat element data".into())
                    })?;
                    next_element += 1;
                    Ok((part.kind, value))
                }
                blueice_ecma402::ListPartKind::Literal => Ok((part.kind, part.value.into())),
            })
            .collect()
    }

    pub(in super::super) fn list_format_format(
        &mut self,
        receiver: &Value,
        list: &Value,
    ) -> Result<Value, RuntimeError> {
        let parts = self.list_format_parts(receiver, list)?;
        let mut result = JsString::default();
        for (_, part) in parts {
            native::append(&mut result, &part, self.config.max_string_bytes)?;
        }
        Ok(Value::String(result))
    }

    pub(in super::super) fn list_format_format_to_parts(
        &mut self,
        receiver: &Value,
        list: &Value,
    ) -> Result<Value, RuntimeError> {
        let parts = self.list_format_parts(receiver, list)?;
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for (kind, value) in parts {
                let part = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                // Keep every completed part rooted until the result Array has
                // taken ownership. A later part allocation may collect the
                // nursery, so a Rust Vec<Value> alone is insufficient.
                self.stack.push(Value::Object(part));
                self.define_data(
                    part,
                    "type",
                    Value::String(
                        match kind {
                            blueice_ecma402::ListPartKind::Element => "element",
                            blueice_ecma402::ListPartKind::Literal => "literal",
                        }
                        .into(),
                    ),
                    true,
                    true,
                    true,
                )?;
                self.define_data(part, "value", Value::String(value), true, true, true)?;
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn list_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.list_format_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        for (key, value) in [
            ("locale", Value::String(resolved.locale.as_str().into())),
            (
                "type",
                Value::String(
                    match resolved.list_type {
                        blueice_ecma402::ListType::Conjunction => "conjunction",
                        blueice_ecma402::ListType::Disjunction => "disjunction",
                        blueice_ecma402::ListType::Unit => "unit",
                    }
                    .into(),
                ),
            ),
            (
                "style",
                Value::String(
                    match resolved.style {
                        blueice_ecma402::ListStyle::Wide => "long",
                        blueice_ecma402::ListStyle::Short => "short",
                        blueice_ecma402::ListStyle::Narrow => "narrow",
                    }
                    .into(),
                ),
            ),
        ] {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
    }

    pub(in super::super) fn duration_style(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::DurationStyle, RuntimeError> {
        match self
            .string_option(options, "style", &["long", "short", "narrow", "digital"])?
            .as_deref()
        {
            None | Some("short") => Ok(blueice_ecma402::DurationStyle::Short),
            Some("long") => Ok(blueice_ecma402::DurationStyle::Long),
            Some("narrow") => Ok(blueice_ecma402::DurationStyle::Narrow),
            Some("digital") => Ok(blueice_ecma402::DurationStyle::Digital),
            Some(_) => unreachable!("string_option validates DurationFormat style"),
        }
    }

    pub(in super::super) fn duration_unit_options(
        &mut self,
        options: &Value,
        unit: blueice_ecma402::DurationUnit,
    ) -> Result<blueice_ecma402::DurationUnitOptions, RuntimeError> {
        let (name, allows_two_digit) = match unit {
            blueice_ecma402::DurationUnit::Years => ("years", false),
            blueice_ecma402::DurationUnit::Months => ("months", false),
            blueice_ecma402::DurationUnit::Weeks => ("weeks", false),
            blueice_ecma402::DurationUnit::Days => ("days", false),
            blueice_ecma402::DurationUnit::Hours => ("hours", true),
            blueice_ecma402::DurationUnit::Minutes => ("minutes", true),
            blueice_ecma402::DurationUnit::Seconds => ("seconds", true),
            blueice_ecma402::DurationUnit::Milliseconds => ("milliseconds", false),
            blueice_ecma402::DurationUnit::Microseconds => ("microseconds", false),
            blueice_ecma402::DurationUnit::Nanoseconds => ("nanoseconds", false),
        };
        let allowed = if allows_two_digit {
            &["long", "short", "narrow", "numeric", "2-digit"][..]
        } else if matches!(
            unit,
            blueice_ecma402::DurationUnit::Milliseconds
                | blueice_ecma402::DurationUnit::Microseconds
                | blueice_ecma402::DurationUnit::Nanoseconds
        ) {
            &["long", "short", "narrow", "numeric"][..]
        } else {
            &["long", "short", "narrow"][..]
        };
        let style = self
            .string_option(options, name, allowed)?
            .map(|style| match style.as_str() {
                "long" => blueice_ecma402::DurationUnitStyle::Long,
                "short" => blueice_ecma402::DurationUnitStyle::Short,
                "narrow" => blueice_ecma402::DurationUnitStyle::Narrow,
                "numeric" => blueice_ecma402::DurationUnitStyle::Numeric,
                "2-digit" => blueice_ecma402::DurationUnitStyle::TwoDigit,
                _ => unreachable!("string_option validates DurationFormat unit style"),
            });
        let display = self
            .string_option(options, &format!("{name}Display"), &["auto", "always"])?
            .map(|display| match display.as_str() {
                "auto" => blueice_ecma402::DurationUnitDisplay::Auto,
                "always" => blueice_ecma402::DurationUnitDisplay::Always,
                _ => unreachable!("string_option validates DurationFormat display"),
            });
        Ok(blueice_ecma402::DurationUnitOptions { style, display })
    }

    pub(in super::super) fn duration_fractional_digits(
        &mut self,
        options: &Value,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &"fractionalDigits".into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite() || value.floor() != value || !(0.0..=9.0).contains(&value) {
            return Err(RuntimeError::RangeError(
                "invalid fractionalDigits option".into(),
            ));
        }
        Ok(Some(value as u8))
    }

    pub(in super::super) fn resolve_duration_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::DurationFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.intl_constructor_options(options)?;
        let locale_matcher = self.locale_matcher(&options)?;
        let requested_numbering_system = self.string_option(&options, "numberingSystem", &[])?;
        if let Some(numbering_system) = requested_numbering_system.as_deref() {
            if !(3..=8).contains(&numbering_system.len())
                || !numbering_system
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric())
            {
                return Err(RuntimeError::RangeError(
                    "invalid numberingSystem option".into(),
                ));
            }
        }
        let locales = locales
            .into_iter()
            .map(|locale| {
                blueice_ecma402::resolve_numbering_system_locale(
                    &locale,
                    requested_numbering_system.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        let style = self.duration_style(&options)?;
        let mut units = [blueice_ecma402::DurationUnitOptions::default(); 10];
        for unit in blueice_ecma402::DurationUnit::ALL {
            units[unit as usize] = self.duration_unit_options(&options, unit)?;
        }
        let options = blueice_ecma402::DurationFormatOptions {
            locale_matcher,
            style,
            units,
            fractional_digits: self.duration_fractional_digits(&options)?,
        };
        blueice_ecma402::DurationFormat::try_new(&locales, options)
            .map(Rc::new)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn duration_format_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let locales = blueice_ecma402::supported_duration_format_locales(
            &locales,
            self.locale_matcher(&options)?,
        );
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    /// Builds an `Intl.DurationFormat` for a caller that is *not* a `new`
    /// expression — `Temporal.Duration.prototype.toLocaleString`, whose
    /// ECMA-402 definition formats through one. The instance takes
    /// `Intl.DurationFormat.prototype` directly, since there is no
    /// `new.target` to derive a prototype from.
    pub(in super::super) fn duration_format_for_locale_string(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.intl_global()?;
        let constructor = self.globals["%Intl.DurationFormat%"];
        let prototype = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.DurationFormat.prototype is an object");
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_duration_format(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_duration_format(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn create_duration_format(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Err(RuntimeError::TypeError(
                "Intl.DurationFormat must be called with new".into(),
            ));
        }
        self.intl_global()?;
        let constructor = self.globals["%Intl.DurationFormat%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.DurationFormat.prototype is an object");
        let prototype = self.constructor_prototype(default)?;
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_duration_format(native::argument(args, 0), native::argument(args, 1))?;
        self.with_roots(|heap| heap.alloc_duration_format(data, prototype))
            .map(Value::Object)
    }

    pub(in super::super) fn duration_format_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::DurationFormat>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.duration_format(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.DurationFormat".into(),
        ))
    }

    pub(in super::super) fn duration_record(
        &mut self,
        value: &Value,
    ) -> Result<blueice_ecma402::DurationRecord, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::Duration {
                    return Ok(*temporal
                        .duration
                        .as_deref()
                        .expect("Temporal.Duration values retain a duration record"));
                }
            }
        }
        if matches!(value, Value::String(_)) {
            let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
                RuntimeError::RangeError("invalid Intl.DurationFormat duration string".into())
            })?;
            return self
                .temporal_value_from_string(TemporalKind::Duration, &source)
                .map(|temporal| {
                    *temporal
                        .duration
                        .as_deref()
                        .expect("Temporal.Duration values retain a duration record")
                });
        }
        if !matches!(value, Value::Object(_)) {
            return Err(RuntimeError::TypeError(
                "Intl.DurationFormat duration must be an object".into(),
            ));
        }
        let mut values = [0.0; 10];
        let mut has_duration_field = false;
        for (index, name) in [
            "years",
            "months",
            "weeks",
            "days",
            "hours",
            "minutes",
            "seconds",
            "milliseconds",
            "microseconds",
            "nanoseconds",
        ]
        .into_iter()
        .enumerate()
        {
            let field = self.get_property(value, &name.into())?;
            if field != Value::Undefined {
                has_duration_field = true;
                values[index] = self.coerce_number(&field)?;
            }
        }
        if !has_duration_field {
            return Err(RuntimeError::TypeError(
                "Intl.DurationFormat duration has no fields".into(),
            ));
        }
        blueice_ecma402::DurationRecord::try_from_f64(
            values[0], values[1], values[2], values[3], values[4], values[5], values[6], values[7],
            values[8], values[9],
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn duration_format_format(
        &mut self,
        receiver: &Value,
        duration: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.duration_format_data(receiver)?;
        let duration = self.duration_record(duration)?;
        data.format(duration)
            .map(|value| Value::String(value.into()))
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn duration_format_format_to_parts(
        &mut self,
        receiver: &Value,
        duration: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.duration_format_data(receiver)?;
        let duration = self.duration_record(duration)?;
        let parts = data
            .format_to_parts(duration)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for source in parts {
                let part = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(part));
                let kind = match source.kind {
                    blueice_ecma402::DurationPartKind::Integer => "integer",
                    blueice_ecma402::DurationPartKind::Decimal => "decimal",
                    blueice_ecma402::DurationPartKind::Fraction => "fraction",
                    blueice_ecma402::DurationPartKind::MinusSign => "minusSign",
                    blueice_ecma402::DurationPartKind::Unit => "unit",
                    blueice_ecma402::DurationPartKind::Literal => "literal",
                };
                self.define_data(part, "type", Value::String(kind.into()), true, true, true)?;
                self.define_data(
                    part,
                    "value",
                    Value::String(source.value.into()),
                    true,
                    true,
                    true,
                )?;
                if let Some(unit) = source.unit {
                    let unit = match unit {
                        blueice_ecma402::DurationUnit::Years => "year",
                        blueice_ecma402::DurationUnit::Months => "month",
                        blueice_ecma402::DurationUnit::Weeks => "week",
                        blueice_ecma402::DurationUnit::Days => "day",
                        blueice_ecma402::DurationUnit::Hours => "hour",
                        blueice_ecma402::DurationUnit::Minutes => "minute",
                        blueice_ecma402::DurationUnit::Seconds => "second",
                        blueice_ecma402::DurationUnit::Milliseconds => "millisecond",
                        blueice_ecma402::DurationUnit::Microseconds => "microsecond",
                        blueice_ecma402::DurationUnit::Nanoseconds => "nanosecond",
                    };
                    self.define_data(part, "unit", Value::String(unit.into()), true, true, true)?;
                }
            }
            self.array_from(self.stack[base..].to_vec())
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super) fn duration_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.duration_format_data(receiver)?;
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let object = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(object));
        self.define_data(
            object,
            "locale",
            Value::String(resolved.locale.clone().into()),
            true,
            true,
            true,
        )?;
        self.define_data(
            object,
            "numberingSystem",
            Value::String(resolved.numbering_system.clone().into()),
            true,
            true,
            true,
        )?;
        let style = match resolved.style {
            blueice_ecma402::DurationStyle::Long => "long",
            blueice_ecma402::DurationStyle::Short => "short",
            blueice_ecma402::DurationStyle::Narrow => "narrow",
            blueice_ecma402::DurationStyle::Digital => "digital",
        };
        self.define_data(
            object,
            "style",
            Value::String(style.into()),
            true,
            true,
            true,
        )?;
        for unit in blueice_ecma402::DurationUnit::ALL {
            let (name, display_name) = match unit {
                blueice_ecma402::DurationUnit::Years => ("years", "yearsDisplay"),
                blueice_ecma402::DurationUnit::Months => ("months", "monthsDisplay"),
                blueice_ecma402::DurationUnit::Weeks => ("weeks", "weeksDisplay"),
                blueice_ecma402::DurationUnit::Days => ("days", "daysDisplay"),
                blueice_ecma402::DurationUnit::Hours => ("hours", "hoursDisplay"),
                blueice_ecma402::DurationUnit::Minutes => ("minutes", "minutesDisplay"),
                blueice_ecma402::DurationUnit::Seconds => ("seconds", "secondsDisplay"),
                blueice_ecma402::DurationUnit::Milliseconds => {
                    ("milliseconds", "millisecondsDisplay")
                }
                blueice_ecma402::DurationUnit::Microseconds => {
                    ("microseconds", "microsecondsDisplay")
                }
                blueice_ecma402::DurationUnit::Nanoseconds => ("nanoseconds", "nanosecondsDisplay"),
            };
            let style = match resolved.unit_style(unit) {
                blueice_ecma402::DurationUnitStyle::Long => "long",
                blueice_ecma402::DurationUnitStyle::Short => "short",
                blueice_ecma402::DurationUnitStyle::Narrow => "narrow",
                blueice_ecma402::DurationUnitStyle::Numeric => "numeric",
                blueice_ecma402::DurationUnitStyle::TwoDigit => "2-digit",
            };
            let display = match resolved.unit_display(unit) {
                blueice_ecma402::DurationUnitDisplay::Auto => "auto",
                blueice_ecma402::DurationUnitDisplay::Always => "always",
            };
            self.define_data(object, name, Value::String(style.into()), true, true, true)?;
            self.define_data(
                object,
                display_name,
                Value::String(display.into()),
                true,
                true,
                true,
            )?;
        }
        if let Some(fractional_digits) = resolved.fractional_digits {
            self.define_data(
                object,
                "fractionalDigits",
                Value::Number(f64::from(fractional_digits)),
                true,
                true,
                true,
            )?;
        }
        Ok(Value::Object(object))
    }
}
