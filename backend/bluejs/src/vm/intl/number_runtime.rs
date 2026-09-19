// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn number_format_data(
        &self,
        value: &Value,
    ) -> Result<Rc<intl::NumberFormat>, RuntimeError> {
        if let Value::Object(id) = value {
            if let Some(data) = self.heap.number_format(*id)? {
                return Ok(data);
            }
        }
        Err(RuntimeError::TypeError(
            "receiver is not an Intl.NumberFormat".into(),
        ))
    }

    /// Implements `UnwrapNumberFormat` for the legacy-facing `format` and
    /// `resolvedOptions` methods. `formatToParts` deliberately requires a
    /// directly branded NumberFormat receiver.
    pub(in super::super) fn unwrap_number_format(
        &mut self,
        value: &Value,
    ) -> Result<ObjectId, RuntimeError> {
        if let Some(id) = value.object_id() {
            if self.heap.number_format(id)?.is_some() {
                return Ok(id);
            }
        } else {
            return Err(RuntimeError::TypeError(
                "receiver is not an Intl.NumberFormat".into(),
            ));
        }

        let fallback_symbol = self.intl_legacy_constructed_symbol.clone().ok_or_else(|| {
            RuntimeError::TypeError("receiver is not an Intl.NumberFormat".into())
        })?;
        // `Get` is intentional: a Proxy around a chained receiver must
        // observe the fallback-symbol lookup before its hidden formatter is
        // brand-checked.
        let fallback = self.get_property(value, &PropertyName::from(fallback_symbol))?;
        let Some(id) = fallback.object_id() else {
            return Err(RuntimeError::TypeError(
                "receiver is not an Intl.NumberFormat".into(),
            ));
        };
        self.heap
            .number_format(id)?
            .is_some()
            .then_some(id)
            .ok_or_else(|| RuntimeError::TypeError("receiver is not an Intl.NumberFormat".into()))
    }

    pub(in super::super) fn number_format_format_getter(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.unwrap_number_format(receiver)?;
        if let Some(function) = self.heap.number_format_format(id) {
            return Ok(Value::Object(function));
        }
        let constructor = self.string_intrinsics()?.0;
        let prototype = self.heap.prototype(constructor)?.unwrap();
        let target = self.with_roots(|heap| {
            heap.alloc_native_function(NativeFunction::NumberFormatFormat, "", prototype)
        })?;
        let bound = crate::heap::BoundFunction {
            target,
            // The getter may have received a legacy chained object. Bind the
            // hidden branded NumberFormat selected by UnwrapNumberFormat.
            this: Value::Object(id),
            args: vec![],
            constructible: false,
        };
        let function = self.with_roots(|heap| heap.alloc_bound_function(bound, Some(prototype)))?;
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
        self.heap.set_number_format_format(id, function);
        Ok(Value::Object(function))
    }

    pub(in super::super) fn number_format_format_to_parts(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.number_format_data(receiver)?;
        let parts = self.number_format_parts(&data, value)?;
        self.number_format_parts_to_value(Ok(parts))
    }

    pub(in super::super) fn number_format_format(
        &mut self,
        receiver: &Value,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.number_format_data(receiver)?;
        let parts = self.number_format_parts(&data, value)?;
        Ok(Value::String(
            parts
                .into_iter()
                .map(|part| part.value)
                .collect::<String>()
                .into(),
        ))
    }

    pub(in super::super) fn number_format_parts(
        &mut self,
        data: &blueice_ecma402::NumberFormat,
        value: &Value,
    ) -> Result<Vec<blueice_ecma402::NumberFormatPart>, RuntimeError> {
        data.format_input_to_parts(self.number_format_input(value)?)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn number_format_input(
        &mut self,
        value: &Value,
    ) -> Result<blueice_ecma402::NumberFormatInput, RuntimeError> {
        let value = self.coerce_primitive(value, "number")?;
        let value = match value {
            Value::BigInt(value) => NumberFormatValue::Decimal(value.to_string()),
            Value::String(value) => {
                let number = crate::primitive::number(&Value::String(value.clone()))?;
                match value.to_utf8() {
                    Ok(value) => exact_decimal_intl_mathematical_value(&value)
                        .unwrap_or(NumberFormatValue::Number(number)),
                    _ => NumberFormatValue::Number(number),
                }
            }
            value => NumberFormatValue::Number(crate::primitive::number(&value)?),
        };
        Ok(match value {
            NumberFormatValue::Decimal(value) => blueice_ecma402::NumberFormatInput::Decimal(value),
            NumberFormatValue::ScientificDecimal {
                significand,
                exponent,
            } => blueice_ecma402::NumberFormatInput::ScientificDecimal {
                significand,
                exponent,
            },
            NumberFormatValue::Number(value) => blueice_ecma402::NumberFormatInput::Number(value),
        })
    }

    pub(in super::super) fn number_format_range_values(
        &mut self,
        start: &Value,
        end: &Value,
    ) -> Result<
        (
            blueice_ecma402::NumberFormatInput,
            blueice_ecma402::NumberFormatInput,
        ),
        RuntimeError,
    > {
        // The range methods require both values. Check first so a missing end
        // takes precedence over an observable conversion of the start value.
        if *start == Value::Undefined || *end == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "number range endpoints must not be undefined".into(),
            ));
        }
        // Convert both values before the host checks for NaN, preserving the
        // specified observable order of ToIntlMathematicalValue.
        Ok((
            self.number_format_input(start)?,
            self.number_format_input(end)?,
        ))
    }

    pub(in super::super) fn number_format_format_range(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.number_format_data(receiver)?;
        let (start, end) = self.number_format_range_values(start, end)?;
        data.format_range_inputs(start, end)
            .map(|formatted| Value::String(formatted.into()))
            .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn number_format_format_range_to_parts(
        &mut self,
        receiver: &Value,
        start: &Value,
        end: &Value,
    ) -> Result<Value, RuntimeError> {
        let data = self.number_format_data(receiver)?;
        let (start, end) = self.number_format_range_values(start, end)?;
        let parts = data
            .format_range_inputs_to_parts(start, end)
            .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        self.number_format_range_parts_to_value(parts)
    }

    pub(in super::super) fn number_format_parts_to_value(
        &mut self,
        parts: Result<Vec<blueice_ecma402::NumberFormatPart>, blueice_ecma402::NumberFormatError>,
    ) -> Result<Value, RuntimeError> {
        let parts = parts.map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        let prototype = self.object_prototype;
        let base = self.stack.len();
        let result = (|| {
            for source in parts {
                let part = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
                self.stack.push(Value::Object(part));
                let kind = Self::number_format_part_kind_name(source.kind);
                self.define_data(part, "type", Value::String(kind.into()), true, true, true)?;
                self.define_data(
                    part,
                    "value",
                    Value::String(source.value.into()),
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

    pub(in super::super) fn number_format_range_parts_to_value(
        &mut self,
        parts: Vec<blueice_ecma402::NumberRangePart>,
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
                    Value::String(Self::number_format_part_kind_name(part.kind).into()),
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
                    blueice_ecma402::NumberRangePartSource::Shared => "shared",
                    blueice_ecma402::NumberRangePartSource::StartRange => "startRange",
                    blueice_ecma402::NumberRangePartSource::EndRange => "endRange",
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

    pub(in super::super) fn number_format_part_kind_name(
        kind: blueice_ecma402::NumberFormatPartKind,
    ) -> &'static str {
        match kind {
            blueice_ecma402::NumberFormatPartKind::MinusSign => "minusSign",
            blueice_ecma402::NumberFormatPartKind::PlusSign => "plusSign",
            blueice_ecma402::NumberFormatPartKind::ApproximatelySign => "approximatelySign",
            blueice_ecma402::NumberFormatPartKind::ExponentSeparator => "exponentSeparator",
            blueice_ecma402::NumberFormatPartKind::ExponentMinusSign => "exponentMinusSign",
            blueice_ecma402::NumberFormatPartKind::ExponentInteger => "exponentInteger",
            blueice_ecma402::NumberFormatPartKind::Integer => "integer",
            blueice_ecma402::NumberFormatPartKind::Group => "group",
            blueice_ecma402::NumberFormatPartKind::Decimal => "decimal",
            blueice_ecma402::NumberFormatPartKind::Fraction => "fraction",
            blueice_ecma402::NumberFormatPartKind::Literal => "literal",
            blueice_ecma402::NumberFormatPartKind::Unit => "unit",
            blueice_ecma402::NumberFormatPartKind::Currency => "currency",
            blueice_ecma402::NumberFormatPartKind::PercentSign => "percentSign",
            blueice_ecma402::NumberFormatPartKind::Compact => "compact",
            blueice_ecma402::NumberFormatPartKind::Nan => "nan",
            blueice_ecma402::NumberFormatPartKind::Infinity => "infinity",
        }
    }

    pub(in super::super) fn number_format_resolved_options(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let id = self.unwrap_number_format(receiver)?;
        let data = self
            .heap
            .number_format(id)?
            .expect("UnwrapNumberFormat returns a branded object");
        let resolved = data.resolved_options();
        let prototype = self.object_prototype;
        let result = self.with_roots(|heap| heap.alloc_object(Some(prototype)))?;
        self.stack.push(Value::Object(result));
        let mut properties = vec![
            ("locale", Value::String(resolved.locale.as_str().into())),
            (
                "numberingSystem",
                Value::String(resolved.numbering_system.as_str().into()),
            ),
            (
                "style",
                Value::String(
                    match resolved.style {
                        blueice_ecma402::NumberFormatStyle::Decimal => "decimal",
                        blueice_ecma402::NumberFormatStyle::Percent => "percent",
                        blueice_ecma402::NumberFormatStyle::Currency => "currency",
                        blueice_ecma402::NumberFormatStyle::Unit => "unit",
                    }
                    .into(),
                ),
            ),
        ];
        if let Some(currency) = data.currency() {
            properties.push(("currency", Value::String(currency.code.clone().into())));
            properties.push((
                "currencyDisplay",
                Value::String(
                    match currency.display {
                        blueice_ecma402::NumberCurrencyDisplay::Symbol => "symbol",
                        blueice_ecma402::NumberCurrencyDisplay::Code => "code",
                        blueice_ecma402::NumberCurrencyDisplay::Name => "name",
                        blueice_ecma402::NumberCurrencyDisplay::NarrowSymbol => "narrowSymbol",
                    }
                    .into(),
                ),
            ));
            properties.push((
                "currencySign",
                Value::String(
                    match currency.sign {
                        blueice_ecma402::NumberCurrencySign::Standard => "standard",
                        blueice_ecma402::NumberCurrencySign::Accounting => "accounting",
                    }
                    .into(),
                ),
            ));
        }
        if let Some(unit) = resolved.unit {
            properties.push(("unit", Value::String(unit.identifier().into())));
            properties.push((
                "unitDisplay",
                Value::String(
                    match resolved.unit_display {
                        blueice_ecma402::NumberUnitDisplay::Short => "short",
                        blueice_ecma402::NumberUnitDisplay::Narrow => "narrow",
                        blueice_ecma402::NumberUnitDisplay::Long => "long",
                    }
                    .into(),
                ),
            ));
        }
        properties.push((
            "minimumIntegerDigits",
            Value::Number(resolved.minimum_integer_digits.into()),
        ));
        let significant_digits = data.significant_digits();
        let mixed_precision = significant_digits.is_some()
            && data.rounding_priority() != blueice_ecma402::NumberRoundingPriority::Auto;
        if significant_digits.is_none() || mixed_precision {
            properties.extend([
                (
                    "minimumFractionDigits",
                    Value::Number(resolved.minimum_fraction_digits.into()),
                ),
                (
                    "maximumFractionDigits",
                    Value::Number(resolved.maximum_fraction_digits.into()),
                ),
            ]);
        }
        if let Some((minimum, maximum)) = significant_digits {
            properties.extend([
                (
                    "minimumSignificantDigits",
                    Value::Number(f64::from(minimum)),
                ),
                (
                    "maximumSignificantDigits",
                    Value::Number(f64::from(maximum)),
                ),
            ]);
        }
        properties.push((
            "useGrouping",
            match resolved.use_grouping {
                blueice_ecma402::NumberGrouping::Auto => Value::String("auto".into()),
                blueice_ecma402::NumberGrouping::Never => Value::Bool(false),
                blueice_ecma402::NumberGrouping::Always => Value::String("always".into()),
                blueice_ecma402::NumberGrouping::Min2 => Value::String("min2".into()),
            },
        ));
        properties.push((
            "notation",
            Value::String(
                match resolved.notation {
                    blueice_ecma402::NumberNotation::Standard => "standard",
                    blueice_ecma402::NumberNotation::Scientific => "scientific",
                    blueice_ecma402::NumberNotation::Engineering => "engineering",
                    blueice_ecma402::NumberNotation::Compact => "compact",
                }
                .into(),
            ),
        ));
        if resolved.notation == blueice_ecma402::NumberNotation::Compact {
            properties.push((
                "compactDisplay",
                Value::String(
                    match data.compact_display() {
                        blueice_ecma402::NumberCompactDisplay::Short => "short",
                        blueice_ecma402::NumberCompactDisplay::Long => "long",
                    }
                    .into(),
                ),
            ));
        }
        properties.push((
            "signDisplay",
            Value::String(
                match resolved.sign_display {
                    blueice_ecma402::NumberSignDisplay::Auto => "auto",
                    blueice_ecma402::NumberSignDisplay::Never => "never",
                    blueice_ecma402::NumberSignDisplay::Always => "always",
                    blueice_ecma402::NumberSignDisplay::ExceptZero => "exceptZero",
                    blueice_ecma402::NumberSignDisplay::Negative => "negative",
                }
                .into(),
            ),
        ));
        properties.extend([
            (
                "roundingIncrement",
                Value::Number(f64::from(data.rounding_increment())),
            ),
            (
                "roundingMode",
                Value::String(
                    match resolved.rounding_mode {
                        blueice_ecma402::NumberRoundingMode::HalfExpand => "halfExpand",
                        blueice_ecma402::NumberRoundingMode::Floor => "floor",
                        blueice_ecma402::NumberRoundingMode::Ceil => "ceil",
                        blueice_ecma402::NumberRoundingMode::Expand => "expand",
                        blueice_ecma402::NumberRoundingMode::Trunc => "trunc",
                        blueice_ecma402::NumberRoundingMode::HalfCeil => "halfCeil",
                        blueice_ecma402::NumberRoundingMode::HalfFloor => "halfFloor",
                        blueice_ecma402::NumberRoundingMode::HalfTrunc => "halfTrunc",
                        blueice_ecma402::NumberRoundingMode::HalfEven => "halfEven",
                    }
                    .into(),
                ),
            ),
            (
                "roundingPriority",
                Value::String(
                    match data.rounding_priority() {
                        blueice_ecma402::NumberRoundingPriority::Auto => "auto",
                        blueice_ecma402::NumberRoundingPriority::MorePrecision => "morePrecision",
                        blueice_ecma402::NumberRoundingPriority::LessPrecision => "lessPrecision",
                    }
                    .into(),
                ),
            ),
            (
                "trailingZeroDisplay",
                Value::String(
                    match data.trailing_zero_display() {
                        blueice_ecma402::NumberTrailingZeroDisplay::Auto => "auto",
                        blueice_ecma402::NumberTrailingZeroDisplay::StripIfInteger => {
                            "stripIfInteger"
                        }
                    }
                    .into(),
                ),
            ),
        ]);
        for (key, value) in properties {
            self.define_data(result, key, value, true, true, true)?;
        }
        Ok(Value::Object(result))
    }
}
