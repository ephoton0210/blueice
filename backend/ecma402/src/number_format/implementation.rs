// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl NumberFormat {
    /// Constructs a decimal formatter after locale negotiation and option
    /// resolution.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: NumberFormatOptions,
    ) -> Result<Self, NumberFormatError> {
        Self::try_new_with_digit_options(
            requested,
            options,
            1,
            None,
            None,
            NumberTrailingZeroDisplay::Auto,
        )
    }

    /// Constructs a formatter with one of ECMA-402's permitted rounding
    /// increments while keeping the common host option record stable.
    pub fn try_new_with_rounding_increment(
        requested: &[CanonicalLocale],
        options: NumberFormatOptions,
        rounding_increment: u16,
    ) -> Result<Self, NumberFormatError> {
        Self::try_new_with_digit_options(
            requested,
            options,
            rounding_increment,
            None,
            None,
            NumberTrailingZeroDisplay::Auto,
        )
    }

    /// Constructs a formatter with ECMA-402 rounding increment and
    /// significant-digit precision options.
    pub fn try_new_with_precision(
        requested: &[CanonicalLocale],
        options: NumberFormatOptions,
        rounding_increment: u16,
        minimum_significant_digits: Option<u8>,
        maximum_significant_digits: Option<u8>,
    ) -> Result<Self, NumberFormatError> {
        Self::try_new_with_digit_options(
            requested,
            options,
            rounding_increment,
            minimum_significant_digits,
            maximum_significant_digits,
            NumberTrailingZeroDisplay::Auto,
        )
    }

    /// Constructs a formatter with all currently supported digit policies.
    pub fn try_new_with_digit_options(
        requested: &[CanonicalLocale],
        options: NumberFormatOptions,
        rounding_increment: u16,
        minimum_significant_digits: Option<u8>,
        maximum_significant_digits: Option<u8>,
        trailing_zero_display: NumberTrailingZeroDisplay,
    ) -> Result<Self, NumberFormatError> {
        Self::try_new_with_currency(
            requested,
            options,
            rounding_increment,
            minimum_significant_digits,
            maximum_significant_digits,
            trailing_zero_display,
            None,
        )
    }

    /// Constructs a formatter with the host-neutral currency record selected
    /// by an embedding's ECMAScript option adapter.
    pub fn try_new_with_currency(
        requested: &[CanonicalLocale],
        options: NumberFormatOptions,
        rounding_increment: u16,
        minimum_significant_digits: Option<u8>,
        maximum_significant_digits: Option<u8>,
        trailing_zero_display: NumberTrailingZeroDisplay,
        currency: Option<NumberCurrencyOptions>,
    ) -> Result<Self, NumberFormatError> {
        Self::try_new_with_construction_options(
            requested,
            options,
            NumberFormatConstructionOptions {
                rounding_increment,
                minimum_significant_digits,
                maximum_significant_digits,
                trailing_zero_display,
                currency,
                numbering_system: None,
            },
        )
    }

    /// Constructs a formatter with a NumberFormat `numberingSystem` option.
    ///
    /// The preference is deliberately separate from the requested locale
    /// list: an explicit option also applies when locale negotiation falls
    /// back to the host default, and must not become visible as a `-u-nu-`
    /// locale extension.
    pub fn try_new_with_construction_options(
        requested: &[CanonicalLocale],
        options: NumberFormatOptions,
        construction: NumberFormatConstructionOptions,
    ) -> Result<Self, NumberFormatError> {
        let NumberFormatConstructionOptions {
            rounding_increment,
            minimum_significant_digits,
            maximum_significant_digits,
            trailing_zero_display,
            currency,
            numbering_system,
        } = construction;
        if rounding_increment_parts(rounding_increment).is_none() {
            return Err(NumberFormatError::InvalidRoundingIncrement);
        }
        if options.style == NumberFormatStyle::Currency && currency.is_none() {
            return Err(NumberFormatError::MissingCurrency);
        }
        let (minimum_fraction_default, maximum_fraction_default) = if options.style
            == NumberFormatStyle::Currency
            && options.notation == NumberNotation::Standard
        {
            let digits = currency.as_ref().map_or(2, |currency| {
                crate::locale_data_provider()
                    .currency_fraction_digits(&currency.code)
                    .unwrap_or(2)
            });
            (digits, digits)
        } else if options.notation == NumberNotation::Compact
            || options.style == NumberFormatStyle::Percent
        {
            (0, 0)
        } else {
            (0, 3)
        };
        let (minimum_fraction_digits, maximum_fraction_digits) = resolve_fraction_digits(
            options,
            rounding_increment,
            minimum_fraction_default,
            maximum_fraction_default,
        )?;
        let significant_digits =
            resolve_significant_digits(minimum_significant_digits, maximum_significant_digits)?;
        if rounding_increment != 1 && significant_digits.is_some() {
            return Err(NumberFormatError::IncompatibleRoundingIncrement);
        }
        if !(1..=21).contains(&options.minimum_integer_digits) {
            return Err(NumberFormatError::MinimumIntegerDigitsOutOfRange);
        }
        if options.style == NumberFormatStyle::Unit && options.unit.is_none() {
            return Err(NumberFormatError::MissingUnit);
        }
        let negotiation = negotiate_number_format_locale(requested, options.locale_matcher);
        let selected =
            resolve_numbering_system_locale(&negotiation.selected, numbering_system.as_deref());
        let numbering_system = unicode_keyword(selected.locale(), "nu")
            .expect("numbering-system resolution always sets the nu key");
        let symbols = locale_data_provider()
            .number_decimal_symbols(selected.locale(), &numbering_system)
            .ok_or(NumberFormatError::DataUnavailable)?;
        let decimal_digits = locale_data_provider()
            .decimal_digits(&numbering_system)
            .ok_or(NumberFormatError::DataUnavailable)?;
        let scientific_symbols = NumberScientificSymbols::from_cldr(
            symbols.exponent_separator.clone(),
            symbols.exponent_minus_sign.clone(),
        );
        let provider = PinnedDecimalProvider::new(symbols, decimal_digits);
        let mut formatter_options = DecimalFormatterOptions::default();
        formatter_options.grouping_strategy = Some(options.use_grouping.into());
        let mut preferences: DecimalFormatterPreferences = selected.locale().into();
        let value = numbering_system
            .parse::<icu_locale_core::extensions::unicode::Value>()
            .map_err(|_| NumberFormatError::DataUnavailable)?;
        preferences.numbering_system =
            Some(NumberingSystem::try_from(value).map_err(|_| NumberFormatError::DataUnavailable)?);
        let formatter =
            DecimalFormatter::try_new_unstable(&provider, preferences, formatter_options)
                .map_err(|_| NumberFormatError::DataUnavailable)?;
        let resolved = ResolvedNumberFormatOptions {
            locale: selected.as_str().into(),
            numbering_system,
            use_grouping: options.use_grouping,
            style: options.style,
            notation: options.notation,
            unit: options.unit,
            unit_display: options.unit_display,
            minimum_integer_digits: options.minimum_integer_digits,
            minimum_fraction_digits,
            maximum_fraction_digits,
            rounding_mode: options.rounding_mode,
            sign_display: options.sign_display,
        };
        let display_plural_rules = (matches!(
            options.style,
            NumberFormatStyle::Unit | NumberFormatStyle::Currency
        ) || options.notation == NumberNotation::Compact)
            .then(|| {
                PluralRules::try_new(
                    std::slice::from_ref(&selected),
                    PluralRulesOptions {
                        locale_matcher: options.locale_matcher,
                        ..Default::default()
                    },
                )
                .map_err(|_| NumberFormatError::DataUnavailable)
            })
            .transpose()?;
        Ok(Self {
            formatter,
            decimal_digits,
            scientific_symbols,
            display_plural_rules,
            negotiation,
            resolved,
            rounding_increment,
            significant_digits,
            rounding_priority: options.rounding_priority,
            trailing_zero_display,
            compact_display: options.compact_display,
            currency,
        })
    }

    /// Formats a finite base-10 decimal string.
    ///
    /// The string uses an optional ASCII sign, ASCII digits and an optional
    /// decimal point. It is a host-neutral boundary that does not expose an
    /// ICU4X decimal type to callers.
    pub fn format_decimal(&self, value: &str) -> Result<String, NumberFormatError> {
        Ok(self
            .format_to_parts_decimal(value)?
            .iter()
            .map(|part| part.value.as_str())
            .collect())
    }

    /// Formats a finite IEEE-754 number using its shortest round-trippable
    /// decimal representation before applying ECMA-402 fraction-digit rules.
    pub fn format_f64(&self, value: f64) -> Result<String, NumberFormatError> {
        Ok(self
            .format_to_parts_f64(value)?
            .iter()
            .map(|part| part.value.as_str())
            .collect())
    }

    /// Formats an input whose ECMAScript numeric kind has already been
    /// selected by the embedding.
    pub fn format_input(&self, value: NumberFormatInput) -> Result<String, NumberFormatError> {
        Ok(self
            .format_input_to_parts(value)?
            .iter()
            .map(|part| part.value.as_str())
            .collect())
    }

    /// Formats a numeric range with range-part provenance retained.
    ///
    /// The decimal service owns range pattern selection so embeddings do not
    /// accidentally lose precision by converting a BigInt or decimal string
    /// to `f64` while assembling a range.
    pub fn format_range_inputs(
        &self,
        start: NumberFormatInput,
        end: NumberFormatInput,
    ) -> Result<String, NumberFormatError> {
        Ok(self
            .format_range_inputs_to_parts(start, end)?
            .iter()
            .map(|part| part.value.as_str())
            .collect())
    }

    /// Formats a numeric range with `formatRangeToParts` provenance.
    pub fn format_range_inputs_to_parts(
        &self,
        start: NumberFormatInput,
        end: NumberFormatInput,
    ) -> Result<Vec<NumberRangePart>, NumberFormatError> {
        if matches!(&start, NumberFormatInput::Number(value) if value.is_nan())
            || matches!(&end, NumberFormatInput::Number(value) if value.is_nan())
        {
            return Err(NumberFormatError::RangeNaN);
        }
        let FormattedNumber {
            parts: mut start_parts,
            numeric_parts: start_numeric_parts,
            display_plural_category: start_plural_category,
            unit_hides_number: start_unit_hides_number,
        } = self.format_input_to_formatted_number(start)?;
        let FormattedNumber {
            parts: mut end_parts,
            numeric_parts: end_numeric_parts,
            display_plural_category: end_plural_category,
            unit_hides_number: end_unit_hides_number,
        } = self.format_input_to_formatted_number(end)?;
        if start_parts == end_parts {
            let mut result = Vec::with_capacity(start_parts.len() + 1);
            result.push(NumberRangePart {
                kind: NumberFormatPartKind::ApproximatelySign,
                value: crate::locale_data_provider()
                    .number_approximately_sign(&self.resolved.locale)
                    .unwrap_or_else(|| "~".into()),
                source: NumberRangePartSource::Shared,
            });
            result.extend(start_parts.drain(..).map(shared_number_range_part));
            return Ok(result);
        }

        let hidden_unit_range_suffix = if self.resolved.style == NumberFormatStyle::Unit
            && (start_unit_hides_number || end_unit_hides_number)
        {
            let range_plural_category = start_plural_category
                .zip(end_plural_category)
                .and_then(|(start, end)| {
                    crate::locale_data_provider().number_range_plural_category(
                        &self.resolved.locale,
                        start,
                        end,
                    )
                })
                .or(end_plural_category);
            range_plural_category.and_then(|range_plural_category| {
                let unit = self.resolved.unit?;
                let mut end_with_range_affix = end_numeric_parts.clone();
                apply_unit_pattern(
                    &mut end_with_range_affix,
                    &self.resolved.locale,
                    unit,
                    self.resolved.unit_display,
                    range_plural_category,
                );
                (end_with_range_affix[..end_numeric_parts.len()] == end_numeric_parts)
                    .then(|| end_with_range_affix.split_off(end_numeric_parts.len()))
            })
        } else {
            None
        };
        if hidden_unit_range_suffix.is_some() {
            start_parts = start_numeric_parts;
            end_parts = end_numeric_parts;
        }

        let currency_range =
            self.resolved.style == NumberFormatStyle::Currency && self.currency.is_some();
        let currency_sign = self.currency.as_ref().map(|currency| currency.sign);
        let range_signs = (
            number_range_sign_context(&start_parts, currency_sign),
            number_range_sign_context(&end_parts, currency_sign),
        );
        let range_signs_match = range_signs.0 == range_signs.1;
        let range_has_sign = range_signs.0 != NumberRangeSignContext::None
            || range_signs.1 != NumberRangeSignContext::None;
        // Currency shares an affix only when the endpoint sign scopes are
        // compatible. Percent and compact displays use that same condition;
        // units retain their shared suffix even when their endpoint signs
        // differ, but then select the full-endpoint connector below.
        let can_collapse_affixes = match self.resolved.style {
            NumberFormatStyle::Currency | NumberFormatStyle::Percent => range_signs_match,
            NumberFormatStyle::Decimal if self.resolved.notation == NumberNotation::Compact => {
                range_signs_match
            }
            _ => true,
        };
        let range_pattern =
            crate::locale_data_provider().number_range_pattern(&self.resolved.locale);

        // CLDR range patterns commonly share a trailing currency, unit, or
        // percent affix. The formatted parts, rather than a language-family
        // table, tell us whether the selected provider pattern placed that
        // affix after the magnitude.
        let has_range_affix = start_parts.iter().chain(&end_parts).any(|part| {
            matches!(
                part.kind,
                NumberFormatPartKind::Currency
                    | NumberFormatPartKind::Unit
                    | NumberFormatPartKind::PercentSign
                    | NumberFormatPartKind::Compact
            )
        });
        // A compact exact-value pattern can be its complete endpoint (for
        // example French long `mille`). It cannot be extracted as a suffix,
        // but still uses the provider's compact range connector.
        let has_exact_compact_endpoint = self.resolved.notation == NumberNotation::Compact
            && (is_exact_compact_range_endpoint(&start_parts)
                || is_exact_compact_range_endpoint(&end_parts));
        let semantic_affix_lengths = range_semantic_affix_suffix_lengths(&start_parts, &end_parts);
        let semantic_affix_differs =
            semantic_affix_lengths.is_some_and(|(start_length, end_length)| {
                start_length != end_length
                    || start_parts[start_parts.len() - start_length..]
                        != end_parts[end_parts.len() - end_length..]
            });
        let suffix_length = if can_collapse_affixes && !semantic_affix_differs {
            common_number_affix_suffix_length(&start_parts, &end_parts)
        } else {
            0
        };
        // An exact-value compact pattern may consist solely of its `compact`
        // part (for example pinned French long `mille`). It is not an affix
        // when extracting it would erase an endpoint altogether.
        let suffix_is_affix = suffix_length > 0
            && suffix_length < start_parts.len()
            && suffix_length < end_parts.len()
            && range_affix_is_collapsible(
                self.resolved.style,
                self.resolved.notation,
                self.resolved.style == NumberFormatStyle::Unit
                    && self.resolved.unit == Some(NumberFormatUnit::Percent)
                    && self.resolved.unit_display != NumberUnitDisplay::Long,
                &start_parts[start_parts.len() - suffix_length..],
                range_signs,
            );
        let mut range_affix_collapsed = false;
        let suffix = if let Some(suffix) = hidden_unit_range_suffix {
            suffix
        } else if suffix_is_affix {
            range_affix_collapsed = true;
            let start_at = start_parts.len() - suffix_length;
            end_parts.truncate(end_parts.len() - suffix_length);
            start_parts.split_off(start_at)
        } else if can_collapse_affixes && semantic_affix_differs {
            if let Some((start_suffix_length, end_suffix_length)) = semantic_affix_lengths {
                if start_suffix_length >= start_parts.len() || end_suffix_length >= end_parts.len()
                {
                    Vec::new()
                } else {
                    range_affix_collapsed = true;
                    // Rebuild the trailing affix with the CLDR plural-range category
                    // over the rounded endpoint values. This normally matches the
                    // end category, but preserves locales with an explicit range
                    // rule as well as French `1–2 mètres`.
                    let start_at = start_parts.len() - start_suffix_length;
                    start_parts.truncate(start_at);
                    let end_at = end_parts.len() - end_suffix_length;
                    let end_suffix = end_parts.split_off(end_at);
                    let range_plural_category = start_plural_category
                        .zip(end_plural_category)
                        .and_then(|(start, end)| {
                            crate::locale_data_provider().number_range_plural_category(
                                &self.resolved.locale,
                                start,
                                end,
                            )
                        });
                    if let Some(range_plural_category) = range_plural_category {
                        let mut end_with_range_affix = end_parts.clone();
                        let rebuilt = match (
                            self.resolved.style,
                            self.resolved.unit,
                            self.currency.as_ref(),
                        ) {
                            (NumberFormatStyle::Unit, Some(unit), _) => {
                                apply_unit_pattern(
                                    &mut end_with_range_affix,
                                    &self.resolved.locale,
                                    unit,
                                    self.resolved.unit_display,
                                    range_plural_category,
                                );
                                true
                            }
                            (NumberFormatStyle::Currency, _, Some(currency)) => {
                                let negative = end_with_range_affix
                                    .iter()
                                    .any(|part| part.kind == NumberFormatPartKind::MinusSign);
                                apply_currency_pattern(
                                    &mut end_with_range_affix,
                                    currency,
                                    &self.resolved.locale,
                                    &self.resolved.numbering_system,
                                    negative,
                                    range_plural_category,
                                );
                                true
                            }
                            _ => false,
                        };
                        if rebuilt {
                            end_with_range_affix.split_off(end_at)
                        } else {
                            end_suffix
                        }
                    } else {
                        end_suffix
                    }
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // RangeCollapse.AUTO can share an explicit sign with a currency,
        // percent, or compact suffix. Units retain endpoint signs while still
        // sharing their trailing label. A bare currency symbol stays
        // endpoint-specific (`$3 – $5`), while an alphabetic prefix such as
        // `US$` can be shared. Accounting parentheses form one sign scope.
        let prefix_length = common_number_part_prefix_length(&start_parts, &end_parts);
        let prefix = &start_parts[..prefix_length];
        let leading_affix_prefix_length =
            common_leading_range_affix_prefix_length(&start_parts, &end_parts);
        let leading_affix_prefix_has_unit = start_parts[..leading_affix_prefix_length]
            .iter()
            .any(|part| part.kind == NumberFormatPartKind::Unit);
        let prefix_has_explicit_sign = prefix.iter().any(|part| {
            matches!(
                part.kind,
                NumberFormatPartKind::MinusSign | NumberFormatPartKind::PlusSign
            )
        });
        let prefix_has_currency = prefix
            .iter()
            .any(|part| part.kind == NumberFormatPartKind::Currency);
        let prefix_has_non_currency_range_affix = prefix.iter().any(|part| {
            matches!(
                part.kind,
                NumberFormatPartKind::Unit
                    | NumberFormatPartKind::PercentSign
                    | NumberFormatPartKind::Compact
            )
        });
        let prefix_is_non_numeric_affix = prefix_has_non_currency_range_affix
            && prefix.iter().all(|part| {
                !matches!(
                    part.kind,
                    NumberFormatPartKind::Integer
                        | NumberFormatPartKind::Group
                        | NumberFormatPartKind::Decimal
                        | NumberFormatPartKind::Fraction
                        | NumberFormatPartKind::ExponentSeparator
                        | NumberFormatPartKind::ExponentMinusSign
                        | NumberFormatPartKind::ExponentInteger
                )
            });
        let prefix_has_alphabetic_currency = prefix.iter().any(|part| {
            part.kind == NumberFormatPartKind::Currency
                && part.value.chars().any(char::is_alphabetic)
        });
        let suffix_has_currency = suffix
            .iter()
            .any(|part| part.kind == NumberFormatPartKind::Currency);
        let accounting_range = matches!(
            range_signs,
            (
                NumberRangeSignContext::Accounting,
                NumberRangeSignContext::Accounting
            )
        );
        let currency_prefix_collapses = currency_range
            && range_signs_match
            && prefix_length > 0
            && (prefix_has_explicit_sign || accounting_range || prefix_has_alphabetic_currency)
            && (prefix_has_currency || suffix_has_currency);
        let non_currency_sign_prefix_collapses = !currency_range
            && range_signs_match
            && range_has_sign
            && range_affix_collapsed
            && prefix_has_explicit_sign
            && (self.resolved.style == NumberFormatStyle::Percent
                || (self.resolved.style == NumberFormatStyle::Unit
                    && self.resolved.unit == Some(NumberFormatUnit::Percent)
                    && self.resolved.unit_display != NumberUnitDisplay::Long)
                || (self.resolved.style == NumberFormatStyle::Decimal
                    && self.resolved.notation == NumberNotation::Compact));
        // Prefix unit labels (such as Japanese `摂氏`) share independently of
        // their suffix. Direct percent/compact prefixes behave like their
        // suffix counterparts: an unsigned bare marker remains on both
        // endpoints, while a shared sign or a locale literal permits
        // collapsing the complete prefix.
        let unit_prefix_collapses = !currency_range
            && self.resolved.style == NumberFormatStyle::Unit
            && leading_affix_prefix_length > 0
            && leading_affix_prefix_has_unit
            && (self.resolved.unit != Some(NumberFormatUnit::Percent)
                || self.resolved.unit_display == NumberUnitDisplay::Long
                || range_affix_is_collapsible(
                    self.resolved.style,
                    self.resolved.notation,
                    true,
                    &start_parts[..leading_affix_prefix_length],
                    range_signs,
                ));
        let direct_prefix_affix_collapses = !currency_range
            && prefix_is_non_numeric_affix
            && (self.resolved.style == NumberFormatStyle::Percent
                || (self.resolved.style == NumberFormatStyle::Decimal
                    && self.resolved.notation == NumberNotation::Compact))
            && range_affix_is_collapsible(
                self.resolved.style,
                self.resolved.notation,
                self.resolved.style == NumberFormatStyle::Unit
                    && self.resolved.unit == Some(NumberFormatUnit::Percent)
                    && self.resolved.unit_display != NumberUnitDisplay::Long,
                prefix,
                range_signs,
            );
        let range_prefix_affix_collapsed = unit_prefix_collapses || direct_prefix_affix_collapses;
        let shared_prefix_length = if currency_prefix_collapses
            || non_currency_sign_prefix_collapses
            || direct_prefix_affix_collapses
        {
            prefix_length
        } else if unit_prefix_collapses {
            leading_affix_prefix_length
        } else {
            0
        };
        let shared_prefix_has_explicit_sign =
            shared_prefix_length == prefix_length && prefix_has_explicit_sign;
        let mut shared_prefix = Vec::new();
        if shared_prefix_length > 0 {
            shared_prefix = start_parts.drain(..shared_prefix_length).collect();
            end_parts.drain(..shared_prefix_length);
        }

        let uses_collapsed_connector = if currency_range {
            range_affix_collapsed || !shared_prefix.is_empty()
        } else if range_has_sign {
            (range_affix_collapsed || range_prefix_affix_collapsed)
                && shared_prefix_has_explicit_sign
        } else {
            !has_range_affix
                || range_affix_collapsed
                || range_prefix_affix_collapsed
                || has_exact_compact_endpoint
        };

        let mut result = Vec::with_capacity(
            shared_prefix.len() + start_parts.len() + end_parts.len() + suffix.len() + 1,
        );
        result.extend(shared_prefix.into_iter().map(shared_number_range_part));
        result.extend(start_parts.into_iter().map(start_number_range_part));
        result.push(NumberRangePart {
            kind: NumberFormatPartKind::Literal,
            value: (if uses_collapsed_connector {
                range_pattern.collapsed_separator
            } else {
                range_pattern.uncollapsed_separator
            })
            .into(),
            source: NumberRangePartSource::Shared,
        });
        result.extend(end_parts.into_iter().map(end_number_range_part));
        result.extend(suffix.into_iter().map(shared_number_range_part));
        Ok(result)
    }

    /// Formats an input into ECMA-402 parts without collapsing its numeric
    /// representation through an embedding-specific conversion.
    pub fn format_input_to_parts(
        &self,
        value: NumberFormatInput,
    ) -> Result<Vec<NumberFormatPart>, NumberFormatError> {
        self.format_input_to_formatted_number(value)
            .map(|formatted| formatted.parts)
    }

    fn format_input_to_formatted_number(
        &self,
        value: NumberFormatInput,
    ) -> Result<FormattedNumber, NumberFormatError> {
        match value {
            NumberFormatInput::Decimal(value) => {
                let value = number_sign_display_decimal(&value, self.resolved.sign_display);
                let decimal =
                    Decimal::try_from_str(value).map_err(|_| NumberFormatError::InvalidDecimal)?;
                self.format_decimal_value(decimal)
            }
            NumberFormatInput::ScientificDecimal {
                significand,
                exponent,
            } => {
                let significand =
                    number_sign_display_decimal(&significand, self.resolved.sign_display);
                let mut decimal = Decimal::try_from_str(significand)
                    .map_err(|_| NumberFormatError::InvalidDecimal)?;
                decimal.multiply_pow10(exponent);
                self.format_decimal_value(decimal)
            }
            NumberFormatInput::Number(value) if !value.is_finite() => Ok(FormattedNumber {
                parts: self.format_non_finite(value),
                numeric_parts: Vec::new(),
                display_plural_category: None,
                unit_hides_number: false,
            }),
            NumberFormatInput::Number(value) => {
                let value = number_sign_display_f64(value, self.resolved.sign_display);
                let decimal = Decimal::try_from_f64(value, FloatPrecision::RoundTrip)
                    .map_err(|_| NumberFormatError::NonFiniteNumber)?;
                self.format_decimal_value(decimal)
            }
        }
    }

    /// Formats an already-coerced finite decimal string into ECMA-402 parts.
    pub fn format_to_parts_decimal(
        &self,
        value: &str,
    ) -> Result<Vec<NumberFormatPart>, NumberFormatError> {
        let value = number_sign_display_decimal(value, self.resolved.sign_display);
        let decimal =
            Decimal::try_from_str(value).map_err(|_| NumberFormatError::InvalidDecimal)?;
        self.format_decimal_value(decimal)
            .map(|formatted| formatted.parts)
    }

    /// Formats a finite IEEE-754 number into ECMA-402 parts.
    pub fn format_to_parts_f64(
        &self,
        value: f64,
    ) -> Result<Vec<NumberFormatPart>, NumberFormatError> {
        if !value.is_finite() {
            return Ok(self.format_non_finite(value));
        }
        let value = number_sign_display_f64(value, self.resolved.sign_display);
        let decimal = Decimal::try_from_f64(value, FloatPrecision::RoundTrip)
            .map_err(|_| NumberFormatError::NonFiniteNumber)?;
        self.format_decimal_value(decimal)
            .map(|formatted| formatted.parts)
    }

    fn format_decimal_value(
        &self,
        mut value: Decimal,
    ) -> Result<FormattedNumber, NumberFormatError> {
        if self.resolved.style == NumberFormatStyle::Percent {
            value.multiply_pow10(2);
        }
        let compact_magnitude = value.nonzero_magnitude_start();
        let compact_scale = (self.resolved.notation == NumberNotation::Compact).then(|| {
            crate::locale_data_provider().compact_number_pattern(
                &self.resolved.locale,
                &self.resolved.numbering_system,
                compact_magnitude,
                self.compact_display,
                PluralCategory::Other,
                None,
            )
        });
        let compact_scale = compact_scale.flatten();
        if let Some(pattern) = &compact_scale {
            value.multiply_pow10(-pattern.divisor);
        }
        let mut exponent = notation_exponent(&value, self.resolved.notation);
        if let Some(exponent) = exponent {
            value.multiply_pow10(-exponent);
        }
        let (increment, position_adjustment) =
            rounding_increment_parts(self.rounding_increment).expect("validated at construction");
        let maximum_fraction_digits = if self.resolved.notation == NumberNotation::Compact
            && self.significant_digits.is_none()
        {
            compact_maximum_fraction_digits(&value)
        } else {
            self.resolved.maximum_fraction_digits
        };
        let use_significant_digits = self
            .significant_digits
            .is_some_and(|(_, maximum)| match self.rounding_priority {
                NumberRoundingPriority::Auto => true,
                NumberRoundingPriority::MorePrecision => {
                    value.nonzero_magnitude_start() - i16::from(maximum) + 1
                        < -(maximum_fraction_digits as i16) + position_adjustment
                }
                NumberRoundingPriority::LessPrecision => {
                    value.nonzero_magnitude_start() - i16::from(maximum) + 1
                        > -(maximum_fraction_digits as i16) + position_adjustment
                }
            });
        if let Some((minimum, maximum)) = self.significant_digits.filter(|_| use_significant_digits)
        {
            value.round_with_mode(
                value.nonzero_magnitude_start() - i16::from(maximum) + 1,
                self.resolved.rounding_mode.fixed_decimal_mode(),
            );
            let minimum_position = value.nonzero_magnitude_start() - i16::from(minimum) + 1;
            value.pad_end(minimum_position);
        } else {
            value.round_with_mode_and_increment(
                -(maximum_fraction_digits as i16) + position_adjustment,
                self.resolved.rounding_mode.fixed_decimal_mode(),
                increment,
            );
            value.pad_end(-(self.resolved.minimum_fraction_digits as i16));
        }
        if let Some(previous_exponent) = exponent {
            let adjustment = notation_exponent(&value, self.resolved.notation)
                .expect("scientific notation always has an exponent");
            if adjustment != 0 {
                value.multiply_pow10(-adjustment);
                exponent = Some(previous_exponent + adjustment);
                // The carry induced by moving a rounded significand is exact;
                // retain its existing precision instead of applying a second
                // option-resolution round.
            }
        }
        if self.trailing_zero_display == NumberTrailingZeroDisplay::StripIfInteger {
            value.trim_end_if_integer();
        }
        value.pad_start(self.resolved.minimum_integer_digits.into());
        let display_plural_category = matches!(
            self.resolved.style,
            NumberFormatStyle::Unit | NumberFormatStyle::Currency
        )
        .then(|| {
            self.display_plural_rules
                .as_ref()
                .expect("unit/currency formatting constructs plural rules")
                .select_decimal(&value.to_string())
        })
        .transpose()
        .map_err(|_| NumberFormatError::FormattingFailed)?;
        let compact_plural_category = compact_scale
            .as_ref()
            .map(|_| {
                self.display_plural_rules
                    .as_ref()
                    .expect("compact formatting constructs plural rules")
                    .select_decimal(&value.to_string())
            })
            .transpose()
            .map_err(|_| NumberFormatError::FormattingFailed)?;
        let compact = compact_scale.as_ref().and_then(|_| {
            crate::locale_data_provider().compact_number_pattern(
                &self.resolved.locale,
                &self.resolved.numbering_system,
                compact_magnitude,
                self.compact_display,
                compact_plural_category.unwrap_or(PluralCategory::Other),
                matches!(
                    self.resolved.style,
                    NumberFormatStyle::Decimal | NumberFormatStyle::Unit
                )
                .then_some(&value),
            )
        });
        let mut collector = NumberPartCollector::default();
        self.formatter
            .format(&value)
            .write_to_parts(&mut collector)
            .map_err(|_| NumberFormatError::FormattingFailed)?;
        // Compact CLDR patterns own their integer skeleton. In particular,
        // the Korean/Japanese ten-thousand patterns do not inherit ordinary
        // decimal grouping after the value has been scaled.
        if compact_scale.is_some() {
            collector
                .parts
                .retain(|part| part.kind != NumberFormatPartKind::Group);
        }
        if compact.as_ref().is_some_and(|pattern| pattern.hides_number) {
            collector.parts.retain(|part| {
                !matches!(
                    part.kind,
                    NumberFormatPartKind::Integer
                        | NumberFormatPartKind::Group
                        | NumberFormatPartKind::Decimal
                        | NumberFormatPartKind::Fraction
                )
            });
        }
        let zero = decimal_is_zero(&value);
        if zero
            && matches!(
                self.resolved.sign_display,
                NumberSignDisplay::ExceptZero | NumberSignDisplay::Negative
            )
        {
            collector
                .parts
                .retain(|part| part.kind != NumberFormatPartKind::MinusSign);
        }
        let negative = collector
            .parts
            .iter()
            .any(|part| part.kind == NumberFormatPartKind::MinusSign);
        let prepend_plus = match self.resolved.sign_display {
            NumberSignDisplay::Always => !negative,
            NumberSignDisplay::ExceptZero => !zero && !negative,
            NumberSignDisplay::Auto | NumberSignDisplay::Never | NumberSignDisplay::Negative => {
                false
            }
        };
        if prepend_plus {
            collector.parts.insert(
                0,
                NumberFormatPart {
                    kind: NumberFormatPartKind::PlusSign,
                    value: "+".into(),
                },
            );
        }
        // Compact notation belongs to the formatted number itself. Currency,
        // percent, and unit patterns therefore wrap the completed compact
        // number rather than leaving its suffix after a trailing affix.
        if let Some(pattern) = compact {
            if !pattern.prefix_leading_literal.is_empty() {
                let index = collector
                    .parts
                    .iter()
                    .take_while(|part| {
                        matches!(
                            part.kind,
                            NumberFormatPartKind::MinusSign
                                | NumberFormatPartKind::PlusSign
                                | NumberFormatPartKind::Literal
                        )
                    })
                    .count();
                collector.parts.insert(
                    index,
                    NumberFormatPart {
                        kind: NumberFormatPartKind::Literal,
                        value: pattern.prefix_leading_literal,
                    },
                );
            }
            if !pattern.prefix.is_empty() {
                let index = collector
                    .parts
                    .iter()
                    .take_while(|part| {
                        matches!(
                            part.kind,
                            NumberFormatPartKind::MinusSign
                                | NumberFormatPartKind::PlusSign
                                | NumberFormatPartKind::Literal
                        )
                    })
                    .count();
                let mut prefix = vec![NumberFormatPart {
                    kind: NumberFormatPartKind::Compact,
                    value: pattern.prefix,
                }];
                if !pattern.prefix_separator.is_empty() {
                    prefix.push(NumberFormatPart {
                        kind: NumberFormatPartKind::Literal,
                        value: pattern.prefix_separator,
                    });
                }
                if !pattern.prefix_trailing_literal.is_empty() {
                    prefix.push(NumberFormatPart {
                        kind: NumberFormatPartKind::Literal,
                        value: pattern.prefix_trailing_literal,
                    });
                }
                collector.parts.splice(index..index, prefix);
            }
            if !pattern.suffix_separator.is_empty() {
                collector.push(NumberFormatPartKind::Literal, &pattern.suffix_separator);
            }
            if !pattern.suffix.is_empty() {
                collector.push(NumberFormatPartKind::Compact, &pattern.suffix);
            }
            if !pattern.suffix_trailing_literal.is_empty() {
                collector.push(
                    NumberFormatPartKind::Literal,
                    &pattern.suffix_trailing_literal,
                );
            }
        }
        // Only a unit pattern that suppresses its numeric placeholder needs
        // the undecorated core later, when `formatRange` reconstructs the
        // shared range-unit affix.  Decimal, percent, and currency formatting
        // are the hot `format()` path used by Test262's finite matrices, so do
        // not allocate and clone their parts solely for that unit-range case.
        let mut numeric_parts = if self.resolved.style == NumberFormatStyle::Unit {
            collector.parts.clone()
        } else {
            Vec::new()
        };
        let unit_hides_number = if let Some(currency) = self.currency.as_ref() {
            apply_currency_pattern(
                &mut collector.parts,
                currency,
                &self.resolved.locale,
                &self.resolved.numbering_system,
                negative,
                display_plural_category.unwrap_or(PluralCategory::Other),
            );
            false
        } else if self.resolved.style == NumberFormatStyle::Percent {
            apply_percent_pattern(&mut collector.parts, &self.resolved.locale);
            false
        } else if let Some(unit) = self.resolved.unit {
            apply_unit_pattern(
                &mut collector.parts,
                &self.resolved.locale,
                unit,
                self.resolved.unit_display,
                display_plural_category.unwrap_or(PluralCategory::Other),
            )
        } else {
            false
        };
        if let Some(exponent) = exponent {
            collector.push(
                NumberFormatPartKind::ExponentSeparator,
                &self.scientific_symbols.exponent_separator,
            );
            if exponent < 0 {
                if !self.scientific_symbols.exponent_minus_prefix.is_empty() {
                    collector.push(
                        NumberFormatPartKind::Literal,
                        &self.scientific_symbols.exponent_minus_prefix,
                    );
                }
                collector.push(
                    NumberFormatPartKind::ExponentMinusSign,
                    &self.scientific_symbols.exponent_minus_sign,
                );
                if !self.scientific_symbols.exponent_minus_suffix.is_empty() {
                    collector.push(
                        NumberFormatPartKind::Literal,
                        &self.scientific_symbols.exponent_minus_suffix,
                    );
                }
            }
            let exponent = exponent.unsigned_abs().to_string();
            collector.push(NumberFormatPartKind::ExponentInteger, &exponent);
        }
        localize_decimal_parts(&mut collector.parts, &self.decimal_digits);
        if !numeric_parts.is_empty() {
            localize_decimal_parts(&mut numeric_parts, &self.decimal_digits);
        }
        Ok(FormattedNumber {
            parts: collector.parts,
            numeric_parts,
            display_plural_category,
            unit_hides_number,
        })
    }

    fn format_non_finite(&self, value: f64) -> Vec<NumberFormatPart> {
        let negative = value.is_sign_negative() && value.is_infinite();
        let sign = match self.resolved.sign_display {
            NumberSignDisplay::Never => None,
            NumberSignDisplay::Auto | NumberSignDisplay::Negative => {
                negative.then_some(NumberFormatPartKind::MinusSign)
            }
            NumberSignDisplay::Always => Some(if negative {
                NumberFormatPartKind::MinusSign
            } else {
                NumberFormatPartKind::PlusSign
            }),
            NumberSignDisplay::ExceptZero => (!value.is_nan()).then_some(if negative {
                NumberFormatPartKind::MinusSign
            } else {
                NumberFormatPartKind::PlusSign
            }),
        };
        let mut parts = Vec::new();
        if let Some(kind) = sign {
            parts.push(NumberFormatPart {
                kind,
                value: if kind == NumberFormatPartKind::MinusSign {
                    "-".into()
                } else {
                    "+".into()
                },
            });
        }
        let (kind, special) = if value.is_nan() {
            (
                NumberFormatPartKind::Nan,
                if self.resolved.locale.starts_with("zh-Hant")
                    || self.resolved.locale.starts_with("zh-TW")
                {
                    "非數值"
                } else {
                    "NaN"
                },
            )
        } else {
            (NumberFormatPartKind::Infinity, "∞")
        };
        parts.push(NumberFormatPart {
            kind,
            value: special.into(),
        });
        if let Some(currency) = self.currency.as_ref() {
            apply_currency_pattern(
                &mut parts,
                currency,
                &self.resolved.locale,
                &self.resolved.numbering_system,
                negative,
                PluralCategory::Other,
            );
        } else if self.resolved.style == NumberFormatStyle::Percent {
            apply_percent_pattern(&mut parts, &self.resolved.locale);
        } else if let Some(unit) = self.resolved.unit {
            apply_unit_pattern(
                &mut parts,
                &self.resolved.locale,
                unit,
                self.resolved.unit_display,
                PluralCategory::Other,
            );
        }
        parts
    }

    /// Returns the data selected during construction.
    pub fn resolved_options(&self) -> &ResolvedNumberFormatOptions {
        &self.resolved
    }

    /// Returns the resolved ECMA-402 `roundingIncrement` option.
    pub fn rounding_increment(&self) -> u16 {
        self.rounding_increment
    }

    /// Returns the resolved significant-digit precision, when requested.
    pub fn significant_digits(&self) -> Option<(u8, u8)> {
        self.significant_digits
    }

    /// Returns the resolved interaction of fraction and significant digits.
    pub fn rounding_priority(&self) -> NumberRoundingPriority {
        self.rounding_priority
    }

    /// Returns the resolved ECMA-402 `trailingZeroDisplay` option.
    pub fn trailing_zero_display(&self) -> NumberTrailingZeroDisplay {
        self.trailing_zero_display
    }

    /// Returns the selected compact-pattern width.
    pub fn compact_display(&self) -> NumberCompactDisplay {
        self.compact_display
    }

    /// Returns the resolved currency record, when `style` is `currency`.
    pub fn currency(&self) -> Option<&NumberCurrencyOptions> {
        self.currency.as_ref()
    }

    /// Returns the locale-negotiation trace produced during construction.
    pub fn negotiation(&self) -> &NumberFormatLocaleNegotiation {
        &self.negotiation
    }

    /// Returns the heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.resolved.locale.len()
            + self.resolved.numbering_system.len()
    }
}
