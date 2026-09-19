// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    pub(in super::super) fn number_grouping(
        &mut self,
        options: &Value,
        notation: blueice_ecma402::NumberNotation,
    ) -> Result<blueice_ecma402::NumberGrouping, RuntimeError> {
        let value = self.get_property(options, &"useGrouping".into())?;
        match value {
            Value::Undefined => Ok(if notation == blueice_ecma402::NumberNotation::Compact {
                blueice_ecma402::NumberGrouping::Min2
            } else {
                blueice_ecma402::NumberGrouping::Auto
            }),
            Value::Bool(true) => Ok(blueice_ecma402::NumberGrouping::Always),
            Value::Bool(false) => Ok(blueice_ecma402::NumberGrouping::Never),
            value => match self
                .coerce_string(&value)?
                .to_utf8()
                .map_err(|_| RuntimeError::RangeError("invalid useGrouping option".into()))?
                .as_str()
            {
                "auto" => Ok(blueice_ecma402::NumberGrouping::Auto),
                "always" => Ok(blueice_ecma402::NumberGrouping::Always),
                "min2" => Ok(blueice_ecma402::NumberGrouping::Min2),
                "false" | "true" => Ok(blueice_ecma402::NumberGrouping::Auto),
                "" | "null" | "0" => Ok(blueice_ecma402::NumberGrouping::Never),
                _ => Err(RuntimeError::RangeError(
                    "invalid useGrouping option".into(),
                )),
            },
        }
    }

    pub(in super::super) fn number_fraction_digits_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let number = self.coerce_number(&value)?;
        if !number.is_finite() || !(0.0..=100.0).contains(&number) {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(number.floor() as u8))
    }

    pub(in super::super) fn number_minimum_integer_digits_option(
        &mut self,
        options: &Value,
    ) -> Result<u8, RuntimeError> {
        let value = self.get_property(options, &"minimumIntegerDigits".into())?;
        if value == Value::Undefined {
            return Ok(1);
        }
        let number = self.coerce_number(&value)?;
        if !number.is_finite() || !(1.0..=21.0).contains(&number) {
            return Err(RuntimeError::RangeError(
                "invalid minimumIntegerDigits option".into(),
            ));
        }
        Ok(number.floor() as u8)
    }

    pub(in super::super) fn number_style(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberFormatStyle, RuntimeError> {
        match self
            .string_option(
                options,
                "style",
                &["decimal", "percent", "currency", "unit"],
            )?
            .as_deref()
        {
            None | Some("decimal") => Ok(blueice_ecma402::NumberFormatStyle::Decimal),
            Some("percent") => Ok(blueice_ecma402::NumberFormatStyle::Percent),
            Some("currency") => Ok(blueice_ecma402::NumberFormatStyle::Currency),
            Some("unit") => Ok(blueice_ecma402::NumberFormatStyle::Unit),
            Some(_) => unreachable!("string_option validates NumberFormat style"),
        }
    }

    pub(in super::super) fn number_unit(
        &mut self,
        options: &Value,
        style: blueice_ecma402::NumberFormatStyle,
    ) -> Result<Option<blueice_ecma402::NumberFormatUnit>, RuntimeError> {
        let unit = self
            .string_option(options, "unit", &[])?
            .as_deref()
            .map(|unit| {
                blueice_ecma402::NumberFormatUnit::parse(unit)
                    .ok_or_else(|| RuntimeError::RangeError("invalid unit option".into()))
            })
            .transpose()?;
        match (style, unit) {
            (
                blueice_ecma402::NumberFormatStyle::Decimal
                | blueice_ecma402::NumberFormatStyle::Percent
                | blueice_ecma402::NumberFormatStyle::Currency,
                _,
            ) => Ok(None),
            (blueice_ecma402::NumberFormatStyle::Unit, None) => Err(RuntimeError::TypeError(
                "unit is required when style is unit".into(),
            )),
            (blueice_ecma402::NumberFormatStyle::Unit, Some(unit)) => Ok(Some(unit)),
        }
    }

    pub(in super::super) fn number_currency(
        &mut self,
        options: &Value,
        style: blueice_ecma402::NumberFormatStyle,
    ) -> Result<Option<blueice_ecma402::NumberCurrencyOptions>, RuntimeError> {
        let code = self.string_option(options, "currency", &[])?;
        let display = match self
            .string_option(
                options,
                "currencyDisplay",
                &["code", "symbol", "narrowSymbol", "name"],
            )?
            .as_deref()
        {
            None | Some("symbol") => blueice_ecma402::NumberCurrencyDisplay::Symbol,
            Some("code") => blueice_ecma402::NumberCurrencyDisplay::Code,
            Some("narrowSymbol") => blueice_ecma402::NumberCurrencyDisplay::NarrowSymbol,
            Some("name") => blueice_ecma402::NumberCurrencyDisplay::Name,
            Some(_) => unreachable!("string_option validates NumberFormat currencyDisplay"),
        };
        let sign = match self
            .string_option(options, "currencySign", &["standard", "accounting"])?
            .as_deref()
        {
            None | Some("standard") => blueice_ecma402::NumberCurrencySign::Standard,
            Some("accounting") => blueice_ecma402::NumberCurrencySign::Accounting,
            Some(_) => unreachable!("string_option validates NumberFormat currencySign"),
        };
        let code = code
            .map(|code| {
                if code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                    Err(RuntimeError::RangeError("invalid currency option".into()))
                } else {
                    Ok(code.to_ascii_uppercase())
                }
            })
            .transpose()?;
        if style != blueice_ecma402::NumberFormatStyle::Currency {
            return Ok(None);
        }
        let code = code.ok_or_else(|| {
            RuntimeError::TypeError("currency is required when style is currency".into())
        })?;
        Ok(Some(blueice_ecma402::NumberCurrencyOptions {
            code,
            display,
            sign,
        }))
    }

    pub(in super::super) fn number_unit_display(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberUnitDisplay, RuntimeError> {
        match self
            .string_option(options, "unitDisplay", &["short", "narrow", "long"])?
            .as_deref()
        {
            None | Some("short") => Ok(blueice_ecma402::NumberUnitDisplay::Short),
            Some("narrow") => Ok(blueice_ecma402::NumberUnitDisplay::Narrow),
            Some("long") => Ok(blueice_ecma402::NumberUnitDisplay::Long),
            Some(_) => unreachable!("string_option validates NumberFormat unitDisplay"),
        }
    }

    pub(in super::super) fn number_rounding_mode(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberRoundingMode, RuntimeError> {
        match self
            .string_option(
                options,
                "roundingMode",
                &[
                    "ceil",
                    "floor",
                    "expand",
                    "trunc",
                    "halfCeil",
                    "halfFloor",
                    "halfExpand",
                    "halfTrunc",
                    "halfEven",
                ],
            )?
            .as_deref()
        {
            None | Some("halfExpand") => Ok(blueice_ecma402::NumberRoundingMode::HalfExpand),
            Some("ceil") => Ok(blueice_ecma402::NumberRoundingMode::Ceil),
            Some("floor") => Ok(blueice_ecma402::NumberRoundingMode::Floor),
            Some("expand") => Ok(blueice_ecma402::NumberRoundingMode::Expand),
            Some("trunc") => Ok(blueice_ecma402::NumberRoundingMode::Trunc),
            Some("halfCeil") => Ok(blueice_ecma402::NumberRoundingMode::HalfCeil),
            Some("halfFloor") => Ok(blueice_ecma402::NumberRoundingMode::HalfFloor),
            Some("halfTrunc") => Ok(blueice_ecma402::NumberRoundingMode::HalfTrunc),
            Some("halfEven") => Ok(blueice_ecma402::NumberRoundingMode::HalfEven),
            Some(_) => unreachable!("string_option validates NumberFormat roundingMode"),
        }
    }

    /// Reads NumberFormat's notation option at its standard observable point.
    /// Formatting extensions consume this resolved value in their own service
    /// slices; retaining it here keeps the constructor boundary spec ordered.
    pub(in super::super) fn number_notation(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberNotation, RuntimeError> {
        match self
            .string_option(
                options,
                "notation",
                &["standard", "scientific", "engineering", "compact"],
            )?
            .as_deref()
        {
            None | Some("standard") => Ok(blueice_ecma402::NumberNotation::Standard),
            Some("scientific") => Ok(blueice_ecma402::NumberNotation::Scientific),
            Some("engineering") => Ok(blueice_ecma402::NumberNotation::Engineering),
            Some("compact") => Ok(blueice_ecma402::NumberNotation::Compact),
            Some(_) => unreachable!("string_option validates NumberFormat notation"),
        }
    }

    /// Validates the Unicode type grammar accepted by the `numberingSystem`
    /// option. The host-neutral locale service applies a supported value when
    /// its numbering-system data is selected.
    pub(in super::super) fn number_numbering_system(
        &mut self,
        options: &Value,
    ) -> Result<Option<String>, RuntimeError> {
        let Some(value) = self.string_option(options, "numberingSystem", &[])? else {
            return Ok(None);
        };
        if !value.split('-').all(|part| {
            (3..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }) {
            return Err(RuntimeError::RangeError(
                "invalid numberingSystem option".into(),
            ));
        }
        Ok(Some(value.to_ascii_lowercase()))
    }

    /// Reads `compactDisplay` even for the non-compact notation branches, as
    /// required by InitializeNumberFormat's observable option sequence.
    pub(in super::super) fn number_compact_display(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberCompactDisplay, RuntimeError> {
        match self
            .string_option(options, "compactDisplay", &["short", "long"])?
            .as_deref()
        {
            None | Some("short") => Ok(blueice_ecma402::NumberCompactDisplay::Short),
            Some("long") => Ok(blueice_ecma402::NumberCompactDisplay::Long),
            Some(_) => unreachable!("string_option validates NumberFormat compactDisplay"),
        }
    }

    pub(in super::super) fn number_rounding_increment(
        &mut self,
        options: &Value,
    ) -> Result<u16, RuntimeError> {
        let value = self.get_property(options, &"roundingIncrement".into())?;
        if value == Value::Undefined {
            return Ok(1);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite() || value.fract() != 0.0 || !(1.0..=5000.0).contains(&value) {
            return Err(RuntimeError::RangeError(
                "invalid roundingIncrement option".into(),
            ));
        }
        let value = value as u16;
        if !matches!(
            value,
            1 | 2 | 5 | 10 | 20 | 25 | 50 | 100 | 200 | 250 | 500 | 1000 | 2000 | 2500 | 5000
        ) {
            return Err(RuntimeError::RangeError(
                "invalid roundingIncrement option".into(),
            ));
        }
        Ok(value)
    }

    pub(in super::super) fn number_trailing_zero_display(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberTrailingZeroDisplay, RuntimeError> {
        match self
            .string_option(options, "trailingZeroDisplay", &["auto", "stripIfInteger"])?
            .as_deref()
        {
            None | Some("auto") => Ok(blueice_ecma402::NumberTrailingZeroDisplay::Auto),
            Some("stripIfInteger") => {
                Ok(blueice_ecma402::NumberTrailingZeroDisplay::StripIfInteger)
            }
            Some(_) => unreachable!("string_option validates NumberFormat trailingZeroDisplay"),
        }
    }

    pub(in super::super) fn number_significant_digits_option(
        &mut self,
        options: &Value,
        name: &str,
    ) -> Result<Option<u8>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite() || value.fract() != 0.0 || !(1.0..=21.0).contains(&value) {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(value as u8))
    }

    pub(in super::super) fn number_rounding_priority(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberRoundingPriority, RuntimeError> {
        match self
            .string_option(
                options,
                "roundingPriority",
                &["auto", "morePrecision", "lessPrecision"],
            )?
            .as_deref()
        {
            None | Some("auto") => Ok(blueice_ecma402::NumberRoundingPriority::Auto),
            Some("morePrecision") => Ok(blueice_ecma402::NumberRoundingPriority::MorePrecision),
            Some("lessPrecision") => Ok(blueice_ecma402::NumberRoundingPriority::LessPrecision),
            Some(_) => unreachable!("string_option validates NumberFormat roundingPriority"),
        }
    }

    pub(in super::super) fn validate_number_precision_options(
        rounding_increment: u16,
        rounding_priority: blueice_ecma402::NumberRoundingPriority,
        minimum_significant_digits: Option<u8>,
        maximum_significant_digits: Option<u8>,
    ) -> Result<(), RuntimeError> {
        if rounding_increment != 1
            && (rounding_priority != blueice_ecma402::NumberRoundingPriority::Auto
                || minimum_significant_digits.is_some()
                || maximum_significant_digits.is_some())
        {
            return Err(RuntimeError::TypeError(
                "roundingIncrement is incompatible with significant-digit rounding".into(),
            ));
        }
        Ok(())
    }

    pub(in super::super) fn number_sign_display(
        &mut self,
        options: &Value,
    ) -> Result<blueice_ecma402::NumberSignDisplay, RuntimeError> {
        match self
            .string_option(
                options,
                "signDisplay",
                &["auto", "never", "always", "exceptZero", "negative"],
            )?
            .as_deref()
        {
            None | Some("auto") => Ok(blueice_ecma402::NumberSignDisplay::Auto),
            Some("never") => Ok(blueice_ecma402::NumberSignDisplay::Never),
            Some("always") => Ok(blueice_ecma402::NumberSignDisplay::Always),
            Some("exceptZero") => Ok(blueice_ecma402::NumberSignDisplay::ExceptZero),
            Some("negative") => Ok(blueice_ecma402::NumberSignDisplay::Negative),
            Some(_) => unreachable!("string_option validates NumberFormat signDisplay"),
        }
    }

    pub(in super::super) fn resolve_number_format(
        &mut self,
        locales: &Value,
        options: &Value,
    ) -> Result<Rc<intl::NumberFormat>, RuntimeError> {
        let locales = self.canonical_locales(locales)?;
        let options = self.number_format_constructor_options(options)?;
        let locale_matcher = self.locale_matcher(&options)?;
        let requested_numbering_system = self.number_numbering_system(&options)?;
        let style = self.number_style(&options)?;
        let currency = self.number_currency(&options, style)?;
        let unit = self.number_unit(&options, style)?;
        let unit_display = self.number_unit_display(&options)?;
        let notation = self.number_notation(&options)?;
        let minimum_integer_digits = self.number_minimum_integer_digits_option(&options)?;
        let minimum_fraction_digits =
            self.number_fraction_digits_option(&options, "minimumFractionDigits")?;
        let maximum_fraction_digits =
            self.number_fraction_digits_option(&options, "maximumFractionDigits")?;
        let minimum_significant_digits =
            self.number_significant_digits_option(&options, "minimumSignificantDigits")?;
        let maximum_significant_digits =
            self.number_significant_digits_option(&options, "maximumSignificantDigits")?;
        let rounding_increment = self.number_rounding_increment(&options)?;
        let rounding_mode = self.number_rounding_mode(&options)?;
        let rounding_priority = self.number_rounding_priority(&options)?;
        let trailing_zero_display = self.number_trailing_zero_display(&options)?;
        let compact_display = self.number_compact_display(&options)?;
        let use_grouping = self.number_grouping(&options, notation)?;
        let sign_display = self.number_sign_display(&options)?;
        Self::validate_number_precision_options(
            rounding_increment,
            rounding_priority,
            minimum_significant_digits,
            maximum_significant_digits,
        )?;
        let options = blueice_ecma402::NumberFormatOptions {
            locale_matcher,
            use_grouping,
            style,
            notation,
            compact_display,
            unit,
            unit_display,
            minimum_integer_digits,
            minimum_fraction_digits,
            maximum_fraction_digits,
            rounding_mode,
            rounding_priority,
            sign_display,
        };
        blueice_ecma402::NumberFormat::try_new_with_construction_options(
            &locales,
            options,
            blueice_ecma402::NumberFormatConstructionOptions {
                rounding_increment,
                minimum_significant_digits,
                maximum_significant_digits,
                trailing_zero_display,
                currency,
                numbering_system: requested_numbering_system,
            },
        )
        .map(Rc::new)
        .map_err(|error| RuntimeError::RangeError(error.to_string()))
    }

    pub(in super::super) fn number_format_supported_locales(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let locales = self.canonical_locales(native::argument(args, 0))?;
        let options = self.intl_options(native::argument(args, 1))?;
        let matcher = self.locale_matcher(&options)?;
        let locales = blueice_ecma402::supported_number_format_locales(&locales, matcher);
        self.array_from(
            locales
                .iter()
                .map(|locale| Value::String(locale.to_string().into()))
                .collect(),
        )
    }

    pub(in super::super) fn create_number_format(
        &mut self,
        receiver: &Value,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        self.intl_global()?;
        let constructor = self.globals["%Intl.NumberFormat%"];
        let default = self
            .heap
            .get(constructor, "prototype")?
            .object_id()
            .expect("Intl.NumberFormat.prototype is an object");
        let legacy_receiver =
            (!construct && self.intl_legacy_receiver(receiver, default)?).then(|| receiver.clone());
        let prototype = if construct {
            self.constructor_prototype(default)?
        } else {
            default
        };
        self.stack.push(Value::Object(prototype));
        let data =
            self.resolve_number_format(native::argument(args, 0), native::argument(args, 1))?;
        let number_format = self.with_roots(|heap| heap.alloc_number_format(data, prototype))?;
        let Some(legacy_receiver) = legacy_receiver else {
            return Ok(Value::Object(number_format));
        };

        let fallback_symbol = self.intl_legacy_fallback_symbol();
        let legacy_id = legacy_receiver
            .object_id()
            .expect("legacy NumberFormat receiver is an object");
        self.stack.push(Value::Object(number_format));
        if !self.object_define_own_property(
            legacy_id,
            fallback_symbol.into(),
            PropertyDescriptor::data(Value::Object(number_format), false, false, false),
        )? {
            return Err(RuntimeError::TypeError(
                "cannot define IntlLegacyConstructedSymbol property".into(),
            ));
        }
        Ok(legacy_receiver)
    }
}
