// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

pub(super) fn number_sign_display_decimal(value: &str, display: NumberSignDisplay) -> &str {
    let magnitude = value.strip_prefix('-').unwrap_or(value);
    match display {
        NumberSignDisplay::Never => magnitude,
        NumberSignDisplay::Auto
        | NumberSignDisplay::Always
        | NumberSignDisplay::ExceptZero
        | NumberSignDisplay::Negative => value,
    }
}

pub(super) fn number_sign_display_f64(value: f64, display: NumberSignDisplay) -> f64 {
    match display {
        NumberSignDisplay::Never => value.abs(),
        NumberSignDisplay::Auto
        | NumberSignDisplay::Always
        | NumberSignDisplay::ExceptZero
        | NumberSignDisplay::Negative => value,
    }
}

pub(super) fn decimal_is_zero(value: &Decimal) -> bool {
    value
        .to_string()
        .trim_start_matches(['-', '+'])
        .bytes()
        .all(|byte| matches!(byte, b'0' | b'.'))
}

/// Maps ICU4X decimal writeable parts to ECMA-402 `formatToParts` records.
#[derive(Default)]
pub(super) struct NumberPartCollector {
    pub(super) parts: Vec<NumberFormatPart>,
    stack: Vec<NumberFormatPartKind>,
}

impl NumberPartCollector {
    pub(super) fn push(&mut self, kind: NumberFormatPartKind, value: &str) {
        if value.is_empty() {
            return;
        }
        if let Some(part) = self.parts.last_mut().filter(|part| part.kind == kind) {
            part.value.push_str(value);
        } else {
            self.parts.push(NumberFormatPart {
                kind,
                value: value.into(),
            });
        }
    }
}

impl std::fmt::Write for NumberPartCollector {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.push(
            self.stack
                .last()
                .copied()
                .unwrap_or(NumberFormatPartKind::Literal),
            value,
        );
        Ok(())
    }
}

impl PartsWrite for NumberPartCollector {
    type SubPartsWrite = Self;

    fn with_part(
        &mut self,
        part: Part,
        mut write: impl FnMut(&mut Self::SubPartsWrite) -> std::fmt::Result,
    ) -> std::fmt::Result {
        let kind = if part == icu_decimal::parts::MINUS_SIGN {
            NumberFormatPartKind::MinusSign
        } else if part == icu_decimal::parts::PLUS_SIGN {
            NumberFormatPartKind::PlusSign
        } else if part == icu_decimal::parts::INTEGER {
            NumberFormatPartKind::Integer
        } else if part == icu_decimal::parts::GROUP {
            NumberFormatPartKind::Group
        } else if part == icu_decimal::parts::DECIMAL {
            NumberFormatPartKind::Decimal
        } else if part == icu_decimal::parts::FRACTION {
            NumberFormatPartKind::Fraction
        } else {
            NumberFormatPartKind::Literal
        };
        self.stack.push(kind);
        let result = write(self);
        self.stack.pop();
        result
    }
}

/// Applies the selected ICU4X `DecimalDigitsV1` payload to handwritten
/// numeric parts while retaining ICU's locale-specific signs, grouping, and
/// decimal separators.
pub(super) fn localize_decimal_parts(parts: &mut [NumberFormatPart], digits: &[char; 10]) {
    for part in parts.iter_mut().filter(|part| {
        matches!(
            part.kind,
            NumberFormatPartKind::Integer
                | NumberFormatPartKind::Fraction
                | NumberFormatPartKind::ExponentInteger
        )
    }) {
        part.value = localize_decimal_digits_impl(&part.value, digits);
    }
}

pub(super) fn notation_exponent(value: &Decimal, notation: NumberNotation) -> Option<i16> {
    match notation {
        NumberNotation::Standard | NumberNotation::Compact => None,
        NumberNotation::Scientific => Some(value.nonzero_magnitude_start()),
        NumberNotation::Engineering => {
            let magnitude = value.nonzero_magnitude_start();
            Some(magnitude - magnitude.rem_euclid(3))
        }
    }
}

/// Compact notation keeps two visible significant positions below ten and no
/// fractional positions at larger magnitudes. This is applied after the CLDR
/// compact scale has been selected, so `9876` becomes `9.9K` while an
/// unscaled `98765` remains an integer.
pub(super) fn compact_maximum_fraction_digits(value: &Decimal) -> u8 {
    let magnitude = value.nonzero_magnitude_start();
    u8::try_from((1 - magnitude).clamp(0, 100)).expect("clamped compact fraction digits")
}

/// Applies one ICU4X `DecimalDigitsV1` payload to ASCII decimal text.
///
/// DurationFormat's numeric substeps use the same helper so an accepted
/// `numberingSystem` affects both direct NumberFormat and duration output.
pub(super) fn localize_decimal_digits_impl(value: &str, digits: &[char; 10]) -> String {
    value
        .chars()
        .map(|character| {
            character
                .to_digit(10)
                .filter(|_| character.is_ascii_digit())
                .map_or(character, |digit| digits[digit as usize])
        })
        .collect()
}

pub(super) fn apply_unit_pattern(
    parts: &mut Vec<NumberFormatPart>,
    locale: &str,
    unit: NumberFormatUnit,
    display: NumberUnitDisplay,
    plural: PluralCategory,
) -> bool {
    if let Some((numerator, denominator)) = unit.compound_parts() {
        if let Some(pattern) = crate::locale_data_provider().number_compound_unit_pattern(
            locale,
            numerator.as_str(),
            denominator.as_str(),
            display,
            plural,
        ) {
            let mut prefix = Vec::new();
            if !pattern.prefix.is_empty() {
                prefix.push(NumberFormatPart {
                    kind: NumberFormatPartKind::Unit,
                    value: pattern.prefix.into(),
                });
            }
            if !pattern.prefix_separator.is_empty() {
                prefix.push(NumberFormatPart {
                    kind: NumberFormatPartKind::Literal,
                    value: pattern.prefix_separator.into(),
                });
            }
            parts.splice(..0, prefix);
            if !pattern.suffix_separator.is_empty() {
                parts.push(NumberFormatPart {
                    kind: NumberFormatPartKind::Literal,
                    value: pattern.suffix_separator.into(),
                });
            }
            parts.push(NumberFormatPart {
                kind: NumberFormatPartKind::Unit,
                value: pattern.suffix.into(),
            });
            return false;
        }

        let pattern = crate::locale_data_provider().number_generic_compound_unit_pattern(
            locale,
            numerator,
            denominator,
            display,
            plural,
        );
        let hides_number = crate::locale_data_provider()
            .generic_compound_unit_hides_number(locale, numerator, display, plural);
        if hides_number {
            parts.retain(|part| {
                !matches!(
                    part.kind,
                    NumberFormatPartKind::Integer
                        | NumberFormatPartKind::Group
                        | NumberFormatPartKind::Decimal
                        | NumberFormatPartKind::Fraction
                )
            });
        }
        let mut prefix = Vec::new();
        if !pattern.prefix.is_empty() {
            prefix.push(NumberFormatPart {
                kind: NumberFormatPartKind::Unit,
                value: pattern.prefix,
            });
        }
        if !pattern.prefix_separator.is_empty() {
            prefix.push(NumberFormatPart {
                kind: NumberFormatPartKind::Literal,
                value: pattern.prefix_separator,
            });
        }
        parts.splice(..0, prefix);
        if !pattern.suffix_separator.is_empty() {
            parts.push(NumberFormatPart {
                kind: NumberFormatPartKind::Literal,
                value: pattern.suffix_separator,
            });
        }
        parts.push(NumberFormatPart {
            kind: NumberFormatPartKind::Unit,
            value: pattern.suffix,
        });
        return hides_number;
    }

    let pattern = crate::locale_data_provider().number_unit_pattern(locale, unit, display, plural);
    if pattern.hides_number {
        parts.retain(|part| {
            !matches!(
                part.kind,
                NumberFormatPartKind::Integer
                    | NumberFormatPartKind::Group
                    | NumberFormatPartKind::Decimal
                    | NumberFormatPartKind::Fraction
            )
        });
    }
    let mut prefix = Vec::new();
    if !pattern.prefix.is_empty() {
        prefix.push(NumberFormatPart {
            kind: NumberFormatPartKind::Unit,
            value: pattern.prefix,
        });
    }
    if !pattern.prefix_separator.is_empty() {
        prefix.push(NumberFormatPart {
            kind: NumberFormatPartKind::Literal,
            value: pattern.prefix_separator,
        });
    }
    parts.splice(..0, prefix);
    if !pattern.suffix_separator.is_empty() {
        parts.push(NumberFormatPart {
            kind: NumberFormatPartKind::Literal,
            value: pattern.suffix_separator,
        });
    }
    parts.push(NumberFormatPart {
        kind: NumberFormatPartKind::Unit,
        value: pattern.suffix,
    });
    pattern.hides_number
}

pub(super) fn resolve_fraction_digits(
    options: NumberFormatOptions,
    rounding_increment: u16,
    minimum_default: u8,
    maximum_default: u8,
) -> Result<(u8, u8), NumberFormatError> {
    let maximum = options.maximum_fraction_digits.unwrap_or_else(|| {
        options
            .minimum_fraction_digits
            .unwrap_or(minimum_default)
            .max(maximum_default)
    });
    let minimum = options
        .minimum_fraction_digits
        .unwrap_or_else(|| minimum_default.min(maximum));
    if minimum > 100 || maximum > 100 {
        return Err(NumberFormatError::FractionDigitsOutOfRange);
    }
    if minimum > maximum {
        return Err(NumberFormatError::IncompatibleFractionDigits);
    }
    if rounding_increment != 1 && minimum != maximum {
        return Err(NumberFormatError::IncompatibleRoundingIncrement);
    }
    Ok((minimum, maximum))
}

pub(super) fn resolve_significant_digits(
    minimum: Option<u8>,
    maximum: Option<u8>,
) -> Result<Option<(u8, u8)>, NumberFormatError> {
    if minimum.is_none() && maximum.is_none() {
        return Ok(None);
    }
    let minimum = minimum.unwrap_or(1);
    let maximum = maximum.unwrap_or(21);
    if !(1..=21).contains(&minimum) || !(1..=21).contains(&maximum) {
        return Err(NumberFormatError::SignificantDigitsOutOfRange);
    }
    if minimum > maximum {
        return Err(NumberFormatError::IncompatibleSignificantDigits);
    }
    Ok(Some((minimum, maximum)))
}

/// Normalizes an ECMA-402 rounding increment for ICU4X fixed-decimal.
///
/// ICU4X represents 1, 2, 5 and 25 at a digit position. Trailing decimal
/// zeroes in ECMA-402's permitted increments therefore move that position.
pub(super) fn rounding_increment_parts(value: u16) -> Option<(RoundingIncrement, i16)> {
    match value {
        1 => Some((RoundingIncrement::MultiplesOf1, 0)),
        2 => Some((RoundingIncrement::MultiplesOf2, 0)),
        5 => Some((RoundingIncrement::MultiplesOf5, 0)),
        10 => Some((RoundingIncrement::MultiplesOf1, 1)),
        20 => Some((RoundingIncrement::MultiplesOf2, 1)),
        25 => Some((RoundingIncrement::MultiplesOf25, 0)),
        50 => Some((RoundingIncrement::MultiplesOf5, 1)),
        100 => Some((RoundingIncrement::MultiplesOf1, 2)),
        200 => Some((RoundingIncrement::MultiplesOf2, 2)),
        250 => Some((RoundingIncrement::MultiplesOf25, 1)),
        500 => Some((RoundingIncrement::MultiplesOf5, 2)),
        1000 => Some((RoundingIncrement::MultiplesOf1, 3)),
        2000 => Some((RoundingIncrement::MultiplesOf2, 3)),
        2500 => Some((RoundingIncrement::MultiplesOf25, 2)),
        5000 => Some((RoundingIncrement::MultiplesOf5, 3)),
        _ => None,
    }
}

pub(super) fn apply_currency_pattern(
    parts: &mut Vec<NumberFormatPart>,
    currency: &NumberCurrencyOptions,
    locale: &str,
    numbering_system: &str,
    negative: bool,
    plural: PluralCategory,
) {
    let pattern = crate::locale_data_provider()
        .number_currency_pattern(
            locale,
            crate::locale_data::NumberCurrencyPatternRequest {
                numbering_system,
                code: &currency.code,
                display: currency.display,
                accounting: currency.sign == NumberCurrencySign::Accounting
                    && currency.display != NumberCurrencyDisplay::Name,
                negative,
                plural,
            },
        )
        .expect("every resolved NumberFormat locale has pinned CLDR currency data");
    if pattern.consumes_decimal_sign {
        parts.retain(|part| part.kind != NumberFormatPartKind::MinusSign);
    }
    let index = parts
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
    let before = pattern
        .before_number
        .into_iter()
        .map(number_currency_pattern_part);
    parts.splice(index..index, before);
    parts.extend(
        pattern
            .after_number
            .into_iter()
            .map(number_currency_pattern_part),
    );
}

pub(super) fn number_currency_pattern_part(piece: NumberCurrencyPatternPiece) -> NumberFormatPart {
    match piece {
        NumberCurrencyPatternPiece::Literal(value) => NumberFormatPart {
            kind: NumberFormatPartKind::Literal,
            value,
        },
        NumberCurrencyPatternPiece::Currency(value) => NumberFormatPart {
            kind: NumberFormatPartKind::Currency,
            value,
        },
        NumberCurrencyPatternPiece::Sign => NumberFormatPart {
            kind: NumberFormatPartKind::MinusSign,
            value: "-".into(),
        },
    }
}

pub(super) fn apply_percent_pattern(parts: &mut Vec<NumberFormatPart>, locale: &str) {
    let sign = parts
        .iter()
        .find(|part| {
            matches!(
                part.kind,
                NumberFormatPartKind::MinusSign | NumberFormatPartKind::PlusSign
            )
        })
        .cloned();
    if let Some(pattern) =
        crate::locale_data_provider().number_percent_pattern(locale, sign.is_some())
    {
        parts.retain(|part| {
            !matches!(
                part.kind,
                NumberFormatPartKind::MinusSign | NumberFormatPartKind::PlusSign
            )
        });
        let mut formatted = pattern
            .before_number
            .into_iter()
            .filter_map(|piece| number_percent_pattern_part(piece, sign.as_ref()))
            .collect::<Vec<_>>();
        formatted.append(parts);
        formatted.extend(
            pattern
                .after_number
                .into_iter()
                .filter_map(|piece| number_percent_pattern_part(piece, sign.as_ref())),
        );
        *parts = formatted;
        return;
    }

    if crate::locale_data_provider().fallback_percent_has_space(locale) {
        parts.push(NumberFormatPart {
            kind: NumberFormatPartKind::Literal,
            value: "\u{a0}".into(),
        });
    }
    parts.push(NumberFormatPart {
        kind: NumberFormatPartKind::PercentSign,
        value: "%".into(),
    });
}

pub(super) fn number_percent_pattern_part(
    piece: NumberPercentPatternPiece,
    sign: Option<&NumberFormatPart>,
) -> Option<NumberFormatPart> {
    match piece {
        NumberPercentPatternPiece::Literal(value) => Some(NumberFormatPart {
            kind: NumberFormatPartKind::Literal,
            value,
        }),
        NumberPercentPatternPiece::PercentSign(value) => Some(NumberFormatPart {
            kind: NumberFormatPartKind::PercentSign,
            value,
        }),
        NumberPercentPatternPiece::Sign => sign.cloned(),
    }
}

pub(super) fn shared_number_range_part(part: NumberFormatPart) -> NumberRangePart {
    NumberRangePart {
        kind: part.kind,
        value: part.value,
        source: NumberRangePartSource::Shared,
    }
}

pub(super) fn start_number_range_part(part: NumberFormatPart) -> NumberRangePart {
    NumberRangePart {
        kind: part.kind,
        value: part.value,
        source: NumberRangePartSource::StartRange,
    }
}

pub(super) fn end_number_range_part(part: NumberFormatPart) -> NumberRangePart {
    NumberRangePart {
        kind: part.kind,
        value: part.value,
        source: NumberRangePartSource::EndRange,
    }
}

/// The sign presentation carried by one fully formatted range endpoint.
///
/// Accounting's parentheses are currency literals in `formatToParts`, so they
/// need a distinct selector state rather than being conflated with unsigned
/// positive values. The other states apply uniformly to decimal, percent,
/// unit, compact, and currency ranges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NumberRangeSignContext {
    None,
    Plus,
    Minus,
    Accounting,
}

pub(super) fn number_range_sign_context(
    parts: &[NumberFormatPart],
    currency_sign: Option<NumberCurrencySign>,
) -> NumberRangeSignContext {
    if parts
        .iter()
        .any(|part| part.kind == NumberFormatPartKind::MinusSign)
    {
        return NumberRangeSignContext::Minus;
    }
    if parts
        .iter()
        .any(|part| part.kind == NumberFormatPartKind::PlusSign)
    {
        return NumberRangeSignContext::Plus;
    }
    if currency_sign == Some(NumberCurrencySign::Accounting)
        && parts
            .iter()
            .any(|part| part.kind == NumberFormatPartKind::Literal && part.value.contains('('))
        && parts
            .iter()
            .any(|part| part.kind == NumberFormatPartKind::Literal && part.value.contains(')'))
    {
        return NumberRangeSignContext::Accounting;
    }
    NumberRangeSignContext::None
}

/// Direct percent and compact affixes repeat on both positive endpoints
/// unless their locale pattern carries a literal. A shared explicit sign
/// makes the full affix collapsible regardless of that direct shape.
pub(super) fn range_affix_is_collapsible(
    style: NumberFormatStyle,
    notation: NumberNotation,
    is_percent_unit: bool,
    suffix: &[NumberFormatPart],
    signs: (NumberRangeSignContext, NumberRangeSignContext),
) -> bool {
    let direct_percent_or_compact = style == NumberFormatStyle::Percent
        || is_percent_unit
        || (style == NumberFormatStyle::Decimal && notation == NumberNotation::Compact);
    if !direct_percent_or_compact {
        return true;
    }
    if matches!(
        signs,
        (NumberRangeSignContext::Minus, NumberRangeSignContext::Minus)
            | (NumberRangeSignContext::Plus, NumberRangeSignContext::Plus)
    ) {
        return true;
    }
    suffix
        .iter()
        .any(|part| part.kind == NumberFormatPartKind::Literal && !part.value.is_empty())
}

pub(super) fn is_exact_compact_range_endpoint(parts: &[NumberFormatPart]) -> bool {
    parts
        .iter()
        .any(|part| part.kind == NumberFormatPartKind::Compact)
        && parts.iter().all(|part| {
            matches!(
                part.kind,
                NumberFormatPartKind::Compact
                    | NumberFormatPartKind::MinusSign
                    | NumberFormatPartKind::PlusSign
            )
        })
}

pub(super) fn common_leading_range_affix_prefix_length(
    start: &[NumberFormatPart],
    end: &[NumberFormatPart],
) -> usize {
    start
        .iter()
        .zip(end)
        .take_while(|(left, right)| {
            left == right
                && matches!(
                    left.kind,
                    NumberFormatPartKind::Literal
                        | NumberFormatPartKind::Unit
                        | NumberFormatPartKind::PercentSign
                        | NumberFormatPartKind::Compact
                )
        })
        .count()
}

pub(super) fn common_number_part_prefix_length(
    start: &[NumberFormatPart],
    end: &[NumberFormatPart],
) -> usize {
    start
        .iter()
        .zip(end)
        .take_while(|(left, right)| left == right)
        .count()
}

pub(super) fn common_number_affix_suffix_length(
    start: &[NumberFormatPart],
    end: &[NumberFormatPart],
) -> usize {
    start
        .iter()
        .rev()
        .zip(end.iter().rev())
        .take_while(|(left, right)| {
            left == right
                && matches!(
                    left.kind,
                    NumberFormatPartKind::Literal
                        | NumberFormatPartKind::Currency
                        | NumberFormatPartKind::Unit
                        | NumberFormatPartKind::PercentSign
                        | NumberFormatPartKind::Compact
                )
        })
        .count()
}

/// Returns the trailing semantic unit or currency affix lengths for each
/// endpoint. A unit's immediately adjacent literal is part of that affix:
/// plural forms may change or remove it (`1 °C` versus `2°C`) even when the
/// unit label itself remains the same. The caller reuses the CLDR
/// plural-range category to format one shared ending.
pub(super) fn range_semantic_affix_suffix_lengths(
    start: &[NumberFormatPart],
    end: &[NumberFormatPart],
) -> Option<(usize, usize)> {
    let (start_last, end_last) = (start.last()?, end.last()?);
    if start_last.kind != end_last.kind
        || !matches!(
            start_last.kind,
            NumberFormatPartKind::Unit | NumberFormatPartKind::Currency
        )
    {
        return None;
    }

    let affix_length = |parts: &[NumberFormatPart]| {
        let mut length = 1;
        while length < parts.len()
            && parts[parts.len() - length - 1].kind == NumberFormatPartKind::Literal
        {
            length += 1;
        }
        length
    };
    let lengths @ (start_length, end_length) = (affix_length(start), affix_length(end));
    (start_length < start.len() && end_length < end.len()).then_some(lengths)
}

/// The exact CLDR decimal symbols and digits selected before formatter
/// construction. This deliberately never delegates to ICU4X's compact baked
/// provider: a missing locale record must fail construction rather than
/// resolving through root data.
pub(super) struct PinnedDecimalProvider {
    symbols: DecimalSymbols<'static>,
    digits: [char; 10],
}

impl PinnedDecimalProvider {
    pub(super) fn new(symbols: NumberDecimalSymbols, digits: [char; 10]) -> Self {
        let strings = DecimalSymbolStrsBuilder {
            minus_sign_prefix: VarZeroCow::new_borrowed(&symbols.minus_sign),
            minus_sign_suffix: VarZeroCow::new_borrowed(""),
            plus_sign_prefix: VarZeroCow::new_borrowed(&symbols.plus_sign),
            plus_sign_suffix: VarZeroCow::new_borrowed(""),
            decimal_separator: VarZeroCow::new_borrowed(&symbols.decimal_separator),
            grouping_separator: VarZeroCow::new_borrowed(&symbols.grouping_separator),
            numsys: VarZeroCow::new_borrowed(&symbols.numbering_system),
        };
        Self {
            symbols: DecimalSymbols {
                strings: VarZeroCow::from_encodeable(&strings),
                grouping_sizes: GroupingSizes {
                    primary: symbols.primary_grouping,
                    secondary: symbols.secondary_grouping,
                    min_grouping: symbols.minimum_grouping,
                },
            },
            digits,
        }
    }
}

impl DataProvider<DecimalSymbolsV1> for PinnedDecimalProvider {
    fn load(
        &self,
        _request: DataRequest,
    ) -> Result<DataResponse<DecimalSymbolsV1>, icu_provider::DataError> {
        Ok(DataResponse {
            metadata: Default::default(),
            payload: DataPayload::from_owned(self.symbols.clone()),
        })
    }
}

impl DataProvider<DecimalDigitsV1> for PinnedDecimalProvider {
    fn load(
        &self,
        _request: DataRequest,
    ) -> Result<DataResponse<DecimalDigitsV1>, icu_provider::DataError> {
        Ok(DataResponse {
            metadata: Default::default(),
            payload: DataPayload::from_owned(self.digits),
        })
    }
}
