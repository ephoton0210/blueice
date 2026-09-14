// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ECMA-402 Duration Format records and their normative validity checks.
//!
//! JavaScript property access and `ToNumber` belong to the embedding runtime.
//! This module receives the resulting numeric fields and owns the Edition 13
//! duration-record invariants shared by `format` and `formatToParts`.

use fixed_decimal::{SignedRoundingMode, UnsignedRoundingMode};
use icu_decimal::input::{Decimal, FloatPrecision};

/// A host-neutral ECMA-402 Duration Record.
///
/// All ten fields are integral and either all non-negative or all non-positive.
/// The record intentionally preserves non-normalized fields: `90` seconds and
/// `1` minute plus `30` seconds are observably distinct format inputs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DurationRecord {
    /// Calendar years.
    pub years: i128,
    /// Calendar months.
    pub months: i128,
    /// Calendar weeks.
    pub weeks: i128,
    /// Calendar days.
    pub days: i128,
    /// Clock hours.
    pub hours: i128,
    /// Clock minutes.
    pub minutes: i128,
    /// Clock seconds.
    pub seconds: i128,
    /// Milliseconds.
    pub milliseconds: i128,
    /// Microseconds.
    pub microseconds: i128,
    /// Nanoseconds.
    pub nanoseconds: i128,
}

/// A failure while converting or validating an ECMA-402 Duration Record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurationRecordError {
    /// A duration field was `NaN` or infinite.
    NonFinite,
    /// A duration field was not an integral mathematical value.
    NonIntegral,
    /// Non-zero duration fields did not have a common sign.
    MixedSign,
    /// A duration field or normalized time span exceeded Edition 13 limits.
    OutOfRange,
}

impl std::fmt::Display for DurationRecordError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite => formatter.write_str("duration fields must be finite"),
            Self::NonIntegral => formatter.write_str("duration fields must be integral"),
            Self::MixedSign => formatter.write_str("duration fields must have a common sign"),
            Self::OutOfRange => formatter.write_str("duration is outside the ECMA-402 range"),
        }
    }
}

impl std::error::Error for DurationRecordError {}

/// The ten duration units in the table order required by ECMA-402.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurationUnit {
    /// Calendar years.
    Years,
    /// Calendar months.
    Months,
    /// Calendar weeks.
    Weeks,
    /// Calendar days.
    Days,
    /// Clock hours.
    Hours,
    /// Clock minutes.
    Minutes,
    /// Clock seconds.
    Seconds,
    /// Milliseconds.
    Milliseconds,
    /// Microseconds.
    Microseconds,
    /// Nanoseconds.
    Nanoseconds,
}

impl DurationUnit {
    /// Units in `GetDurationUnitOptions` table order.
    pub const ALL: [Self; 10] = [
        Self::Years,
        Self::Months,
        Self::Weeks,
        Self::Days,
        Self::Hours,
        Self::Minutes,
        Self::Seconds,
        Self::Milliseconds,
        Self::Microseconds,
        Self::Nanoseconds,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Years => 0,
            Self::Months => 1,
            Self::Weeks => 2,
            Self::Days => 3,
            Self::Hours => 4,
            Self::Minutes => 5,
            Self::Seconds => 6,
            Self::Milliseconds => 7,
            Self::Microseconds => 8,
            Self::Nanoseconds => 9,
        }
    }
}

/// The global `style` option of `Intl.DurationFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DurationStyle {
    /// Full unit names.
    Long,
    /// Abbreviated unit names.
    #[default]
    Short,
    /// Narrow unit names.
    Narrow,
    /// Clock-style time units with short calendar units.
    Digital,
}

/// A resolved per-unit style.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurationUnitStyle {
    /// Full unit names.
    Long,
    /// Abbreviated unit names.
    Short,
    /// Narrow unit names.
    Narrow,
    /// An ungrouped numeric unit.
    Numeric,
    /// An ungrouped numeric unit padded to at least two digits.
    TwoDigit,
}

impl From<DurationStyle> for DurationUnitStyle {
    fn from(value: DurationStyle) -> Self {
        match value {
            DurationStyle::Long => Self::Long,
            DurationStyle::Short | DurationStyle::Digital => Self::Short,
            DurationStyle::Narrow => Self::Narrow,
        }
    }
}

/// The per-unit `Display` option.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DurationUnitDisplay {
    /// Omit a zero-valued unit unless a numeric separator requires it.
    #[default]
    Auto,
    /// Render the unit even when its value is zero.
    Always,
}

/// Typed host input for one duration unit's style and display options.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DurationUnitOptions {
    /// An explicit style, or the global/default style when absent.
    pub style: Option<DurationUnitStyle>,
    /// An explicit zero-unit display policy, or the normative default when
    /// absent.
    ///
    /// `auto` is observably different from an omitted display option for a
    /// numeric unit, whose default is often `always`; retaining that absence
    /// is therefore required at this host boundary.
    pub display: Option<DurationUnitDisplay>,
}

/// Typed host input for the option-resolution portion of DurationFormat.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DurationFormatOptions {
    /// The requested locale matching policy.
    pub locale_matcher: crate::LocaleMatcher,
    /// The global `style` option.
    pub style: DurationStyle,
    /// Per-unit style/display options in [`DurationUnit::ALL`] order.
    pub units: [DurationUnitOptions; 10],
    /// An optional fractional-second precision from zero through nine.
    pub fractional_digits: Option<u8>,
}

impl Default for DurationFormatOptions {
    fn default() -> Self {
        Self {
            locale_matcher: crate::LocaleMatcher::default(),
            style: DurationStyle::Short,
            units: [DurationUnitOptions::default(); 10],
            fractional_digits: None,
        }
    }
}

/// The resolved host-neutral duration formatting options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedDurationFormatOptions {
    /// The resolved global style.
    pub style: DurationStyle,
    /// Resolved unit styles in [`DurationUnit::ALL`] order.
    pub unit_styles: [DurationUnitStyle; 10],
    /// Resolved unit displays in [`DurationUnit::ALL`] order.
    pub unit_displays: [DurationUnitDisplay; 10],
    /// The optional fractional-second precision.
    pub fractional_digits: Option<u8>,
}

impl ResolvedDurationFormatOptions {
    /// Returns the selected style for `unit`.
    pub fn unit_style(&self, unit: DurationUnit) -> DurationUnitStyle {
        self.unit_styles[unit.index()]
    }

    /// Returns the selected display policy for `unit`.
    pub fn unit_display(&self, unit: DurationUnit) -> DurationUnitDisplay {
        self.unit_displays[unit.index()]
    }
}

/// An invalid typed DurationFormat option combination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurationFormatOptionsError {
    /// A unit did not permit its requested numeric style.
    UnsupportedUnitStyle,
    /// A non-numeric unit followed an already-selected numeric unit.
    IncompatibleUnitStyles,
    /// A fractional-second unit cannot be displayed with `always`.
    FractionalUnitAlwaysDisplay,
    /// A non-fractional unit followed a fractional-second unit.
    IncompatibleFractionalUnitStyles,
    /// `fractionalDigits` was greater than nine.
    FractionalDigitsOutOfRange,
}

impl std::fmt::Display for DurationFormatOptionsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedUnitStyle => formatter.write_str("unsupported duration unit style"),
            Self::IncompatibleUnitStyles => {
                formatter.write_str("duration unit style follows a numeric unit")
            }
            Self::FractionalUnitAlwaysDisplay => {
                formatter.write_str("fractional duration units cannot always be displayed")
            }
            Self::IncompatibleFractionalUnitStyles => {
                formatter.write_str("duration unit style follows a fractional unit")
            }
            Self::FractionalDigitsOutOfRange => {
                formatter.write_str("fractional digits must be between zero and nine")
            }
        }
    }
}

impl std::error::Error for DurationFormatOptionsError {}

/// The type of one `Intl.DurationFormat.prototype.formatToParts` record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurationPartKind {
    /// A localized integer digit sequence.
    Integer,
    /// A localized decimal separator.
    Decimal,
    /// A localized fractional digit sequence.
    Fraction,
    /// The negative sign on the first displayed duration unit.
    MinusSign,
    /// A localized unit name or abbreviation.
    Unit,
    /// A duration separator or list-pattern literal.
    Literal,
}

/// One host-neutral `Intl.DurationFormat.prototype.formatToParts` record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurationPart {
    /// The part classification required by ECMA-402.
    pub kind: DurationPartKind,
    /// The UTF-8 contents of the part.
    pub value: String,
    /// The singular duration unit carried by non-list number and unit parts.
    ///
    /// List separators deliberately carry no unit, matching the observable
    /// JavaScript result object.
    pub unit: Option<DurationUnit>,
}

/// Data and option resolution failure while constructing or using a duration
/// formatter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurationFormatError {
    /// A typed DurationFormat option record was invalid.
    InvalidOptions(DurationFormatOptionsError),
    /// A caller supplied a manually-built record that violates the Duration
    /// Record invariants.
    InvalidDuration(DurationRecordError),
    /// CLDR list-pattern data was unavailable for the selected locale.
    ListFormattingUnavailable,
}

impl std::fmt::Display for DurationFormatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOptions(error) => error.fmt(formatter),
            Self::InvalidDuration(error) => error.fmt(formatter),
            Self::ListFormattingUnavailable => {
                formatter.write_str("duration list-pattern data is unavailable")
            }
        }
    }
}

impl std::error::Error for DurationFormatError {}

/// ECMAScript-observable data resolved by a duration formatter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDurationFormatServiceOptions {
    /// The negotiated locale selected for duration data.
    pub locale: String,
    /// The selected decimal numbering system.
    pub numbering_system: String,
    /// The global duration style.
    pub style: DurationStyle,
    /// Resolved styles in [`DurationUnit::ALL`] order.
    pub unit_styles: [DurationUnitStyle; 10],
    /// Resolved zero-display policies in [`DurationUnit::ALL`] order.
    pub unit_displays: [DurationUnitDisplay; 10],
    /// The optional fractional-second precision.
    pub fractional_digits: Option<u8>,
}

impl ResolvedDurationFormatServiceOptions {
    /// Returns the resolved style for one unit.
    pub fn unit_style(&self, unit: DurationUnit) -> DurationUnitStyle {
        self.unit_styles[unit.index()]
    }

    /// Returns the resolved display policy for one unit.
    pub fn unit_display(&self, unit: DurationUnit) -> DurationUnitDisplay {
        self.unit_displays[unit.index()]
    }
}

/// A host-neutral `Intl.DurationFormat` service.
///
/// The service owns option resolution, numeric grouping, list partitioning and
/// all `formatToParts` boundaries. ECMAScript property lookup and `ToNumber`
/// remain at the embedding boundary. Typed Temporal values and ISO duration
/// strings enter through the VM's single duration-record bridge.
pub struct DurationFormat {
    list_format: crate::ListFormat,
    resolved: ResolvedDurationFormatServiceOptions,
}

impl DurationFormat {
    /// Constructs a duration formatter from canonical requested locales and
    /// typed ECMA-402 options.
    pub fn try_new(
        requested: &[crate::CanonicalLocale],
        options: DurationFormatOptions,
    ) -> Result<Self, DurationFormatError> {
        let requested = duration_supported_requests(requested, options.locale_matcher)
            .into_iter()
            .map(|locale| crate::resolve_numbering_system_locale(&locale, None))
            .collect::<Vec<_>>();
        let list_format = crate::ListFormat::try_new(
            &requested,
            crate::ListFormatOptions {
                locale_matcher: options.locale_matcher,
                list_type: crate::ListType::Unit,
                style: duration_list_style(options.style),
            },
        )
        .map_err(|_| DurationFormatError::ListFormattingUnavailable)?;
        let selected = list_format.negotiation().selected();
        let locale = selected.as_str().to_owned();
        let numbering_system = crate::unicode_keyword(selected.locale(), "nu")
            .filter(|value| crate::supports_numbering_system(value))
            .unwrap_or_else(|| {
                crate::locale_data_provider()
                    .default_numbering_system(selected.locale())
                    .into()
            });
        let options = resolve_duration_format_options_with_digital_format(options, false)
            .map_err(DurationFormatError::InvalidOptions)?;
        Ok(Self {
            list_format,
            resolved: ResolvedDurationFormatServiceOptions {
                locale,
                numbering_system,
                style: options.style,
                unit_styles: options.unit_styles,
                unit_displays: options.unit_displays,
                fractional_digits: options.fractional_digits,
            },
        })
    }

    /// Formats a validated Duration Record into a localized string.
    pub fn format(&self, duration: DurationRecord) -> Result<String, DurationFormatError> {
        Ok(self
            .format_to_parts(duration)?
            .into_iter()
            .map(|part| part.value)
            .collect())
    }

    /// Partitions a validated Duration Record according to Edition 13's
    /// `PartitionDurationFormatPattern` algorithm.
    pub fn format_to_parts(
        &self,
        duration: DurationRecord,
    ) -> Result<Vec<DurationPart>, DurationFormatError> {
        duration
            .validate()
            .map_err(DurationFormatError::InvalidDuration)?;
        let mut partitioned = Vec::<Vec<DurationPart>>::new();
        let mut sign_displayed = false;
        let mut numeric_unit_found = false;
        for unit in DurationUnit::ALL {
            let style = self.resolved.unit_style(unit);
            if matches!(
                style,
                DurationUnitStyle::Numeric | DurationUnitStyle::TwoDigit
            ) {
                let numeric = self.format_numeric_units(duration, unit, &mut sign_displayed);
                if !numeric.is_empty() {
                    partitioned.push(numeric);
                }
                numeric_unit_found = true;
                break;
            }

            let next_is_fractional = duration_next_unit_is_fractional(&self.resolved, unit);
            let value = duration.value(unit);
            if let Some(fractional_unit) =
                duration_fractional_unit(unit).filter(|_| next_is_fractional)
            {
                let total = duration.fractional_total(fractional_unit);
                if total != 0 || self.resolved.unit_display(unit) == DurationUnitDisplay::Always {
                    partitioned.push(self.format_standalone_fractional(
                        unit,
                        total,
                        fractional_unit.exponent(),
                        style,
                        duration.sign() < 0,
                        &mut sign_displayed,
                    ));
                }
                numeric_unit_found = true;
                break;
            }
            if value != 0 || self.resolved.unit_display(unit) == DurationUnitDisplay::Always {
                partitioned.push(self.format_standalone_integer(
                    unit,
                    value,
                    style,
                    duration.sign() < 0,
                    &mut sign_displayed,
                ));
            }
        }
        debug_assert!(numeric_unit_found || !partitioned.is_empty() || duration.sign() == 0);
        self.list_format_parts(partitioned)
    }

    /// Returns the ECMAScript-observable resolved options.
    pub fn resolved_options(&self) -> &ResolvedDurationFormatServiceOptions {
        &self.resolved
    }

    /// Returns the deterministic locale negotiation trace.
    pub fn negotiation(&self) -> &crate::ListFormatLocaleNegotiation {
        self.list_format.negotiation()
    }

    /// Returns directly owned heap storage, excluding ICU4X allocation.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.resolved.locale.len()
            + self.resolved.numbering_system.len()
            + self.list_format.bytes()
    }

    fn format_standalone_integer(
        &self,
        unit: DurationUnit,
        value: i128,
        style: DurationUnitStyle,
        duration_is_negative: bool,
        sign_displayed: &mut bool,
    ) -> Vec<DurationPart> {
        let mut result = self.integer_parts(
            value,
            unit,
            false,
            true,
            duration_should_display_sign(value, duration_is_negative, sign_displayed),
        );
        *sign_displayed = true;
        self.append_unit_pattern(&mut result, unit, style, value.unsigned_abs() == 1);
        result
    }

    fn format_standalone_fractional(
        &self,
        unit: DurationUnit,
        total: i128,
        exponent: u8,
        style: DurationUnitStyle,
        duration_is_negative: bool,
        sign_displayed: &mut bool,
    ) -> Vec<DurationPart> {
        let mut result = self.fractional_parts(
            total,
            exponent,
            unit,
            true,
            false,
            duration_should_display_sign(total, duration_is_negative, sign_displayed),
        );
        *sign_displayed = true;
        self.append_unit_pattern(
            &mut result,
            unit,
            style,
            duration_fraction_is_one(total, exponent),
        );
        result
    }

    fn format_numeric_units(
        &self,
        duration: DurationRecord,
        first: DurationUnit,
        sign_displayed: &mut bool,
    ) -> Vec<DurationPart> {
        debug_assert!(matches!(
            first,
            DurationUnit::Hours | DurationUnit::Minutes | DurationUnit::Seconds
        ));
        let hours = duration.hours;
        let minutes = duration.minutes;
        let seconds = duration.fractional_total(FractionalDurationUnit::Seconds);
        let hours_formatted = first == DurationUnit::Hours
            && (hours != 0
                || self.resolved.unit_display(DurationUnit::Hours) == DurationUnitDisplay::Always);
        let seconds_formatted = seconds != 0
            || self.resolved.unit_display(DurationUnit::Seconds) == DurationUnitDisplay::Always;
        let minutes_formatted = matches!(first, DurationUnit::Hours | DurationUnit::Minutes)
            && ((hours_formatted && seconds_formatted)
                || minutes != 0
                || self.resolved.unit_display(DurationUnit::Minutes)
                    == DurationUnitDisplay::Always);

        let mut result = Vec::new();
        if hours_formatted {
            result.extend(self.integer_parts(
                hours,
                DurationUnit::Hours,
                self.resolved.unit_style(DurationUnit::Hours) == DurationUnitStyle::TwoDigit,
                false,
                duration_should_display_sign(hours, duration.sign() < 0, sign_displayed),
            ));
            *sign_displayed = true;
        }
        if minutes_formatted {
            if hours_formatted {
                result.push(duration_literal(":"));
            }
            result.extend(self.integer_parts(
                minutes,
                DurationUnit::Minutes,
                self.resolved.unit_style(DurationUnit::Minutes) == DurationUnitStyle::TwoDigit,
                false,
                duration_should_display_sign(minutes, duration.sign() < 0, sign_displayed),
            ));
            *sign_displayed = true;
        }
        if seconds_formatted {
            if minutes_formatted {
                result.push(duration_literal(":"));
            }
            result.extend(self.fractional_parts(
                seconds,
                9,
                DurationUnit::Seconds,
                false,
                self.resolved.unit_style(DurationUnit::Seconds) == DurationUnitStyle::TwoDigit,
                duration_should_display_sign(seconds, duration.sign() < 0, sign_displayed),
            ));
        }
        result
    }

    fn integer_parts(
        &self,
        value: i128,
        unit: DurationUnit,
        two_digit: bool,
        grouping: bool,
        display_sign: bool,
    ) -> Vec<DurationPart> {
        let mut result = Vec::new();
        // `PartitionDurationFormatPattern` deliberately passes negative zero
        // to the first displayed NumberFormat value when an earlier field is
        // zero but the duration as a whole is negative. `display_sign`
        // already encodes that decision, so do not derive it from this
        // normalized integer field again.
        if display_sign {
            result.push(DurationPart {
                kind: DurationPartKind::MinusSign,
                value: "-".into(),
                unit: Some(unit),
            });
        }
        let mut digits = value.unsigned_abs().to_string();
        if two_digit && digits.len() < 2 {
            digits.insert(0, '0');
        }
        if grouping {
            digits = english_group_digits(&digits);
        }
        digits = crate::number_format::localize_simple_numbering_system(
            &digits,
            &self.resolved.numbering_system,
        );
        result.push(DurationPart {
            kind: DurationPartKind::Integer,
            value: digits,
            unit: Some(unit),
        });
        result
    }

    fn fractional_parts(
        &self,
        total: i128,
        exponent: u8,
        unit: DurationUnit,
        grouping: bool,
        two_digit: bool,
        display_sign: bool,
    ) -> Vec<DurationPart> {
        let scale = 10_i128.pow(exponent.into());
        // `PartitionDurationFormatPattern` supplies the mathematical total to
        // `PartitionNumberPattern`. The latter is an ECMAScript Number
        // operation, so its observable decimal representation is the finite
        // IEEE-754 value, not an invented arbitrary-precision decimal. Keep
        // record validation exact, then reproduce NumberFormat's truncation
        // to the configured (or default nine) fraction digits.
        let fraction_digits = self.resolved.fractional_digits.unwrap_or(exponent);
        // Convert the integer and fractional components separately. Casting
        // the nanosecond total first would erase a low-order fraction before
        // IEEE-754 gets the chance to round the final Number.
        let quotient = total / scale;
        let remainder = total % scale;
        let value = quotient as f64 + remainder as f64 / scale as f64;
        let mut number = Decimal::try_from_f64(value.abs(), FloatPrecision::RoundTrip)
            .expect("a validated DurationRecord has a finite normalized Number");
        number.round_with_mode(
            -(i16::from(fraction_digits)),
            SignedRoundingMode::Unsigned(UnsignedRoundingMode::Trunc),
        );
        let number = number.to_string();
        let (integer, fraction) = number
            .split_once('.')
            .map_or((number.as_str(), ""), |(integer, fraction)| {
                (integer, fraction)
            });
        let mut result = Vec::new();
        // See `integer_parts`: a zero first numeric field can carry the
        // duration's sole negative sign.
        if display_sign {
            result.push(DurationPart {
                kind: DurationPartKind::MinusSign,
                value: "-".into(),
                unit: Some(unit),
            });
        }
        let mut integer = integer.to_owned();
        if two_digit && integer.len() < 2 {
            integer.insert(0, '0');
        }
        if grouping {
            integer = english_group_digits(&integer);
        }
        integer = crate::number_format::localize_simple_numbering_system(
            &integer,
            &self.resolved.numbering_system,
        );
        result.push(DurationPart {
            kind: DurationPartKind::Integer,
            value: integer,
            unit: Some(unit),
        });
        let fraction = crate::number_format::localize_simple_numbering_system(
            &duration_fraction_digits(fraction, fraction_digits, self.resolved.fractional_digits),
            &self.resolved.numbering_system,
        );
        if !fraction.is_empty() {
            result.push(DurationPart {
                kind: DurationPartKind::Decimal,
                value: ".".into(),
                unit: Some(unit),
            });
            result.push(DurationPart {
                kind: DurationPartKind::Fraction,
                value: fraction,
                unit: Some(unit),
            });
        }
        result
    }

    fn append_unit_pattern(
        &self,
        parts: &mut Vec<DurationPart>,
        unit: DurationUnit,
        style: DurationUnitStyle,
        singular: bool,
    ) {
        let (separator, label) = crate::locale_data_provider().duration_unit_pattern(
            &self.resolved.locale,
            unit,
            style,
            singular,
        );
        if !separator.is_empty() {
            parts.push(DurationPart {
                kind: DurationPartKind::Literal,
                value: separator.into(),
                unit: Some(unit),
            });
        }
        parts.push(DurationPart {
            kind: DurationPartKind::Unit,
            value: label.into(),
            unit: Some(unit),
        });
    }

    fn list_format_parts(
        &self,
        partitioned: Vec<Vec<DurationPart>>,
    ) -> Result<Vec<DurationPart>, DurationFormatError> {
        let values = partitioned
            .iter()
            .map(|parts| {
                parts
                    .iter()
                    .map(|part| part.value.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let list_parts = self
            .list_format
            .format_to_parts(values.iter())
            .map_err(|_| DurationFormatError::ListFormattingUnavailable)?;
        let mut next_element = partitioned.into_iter();
        let mut flattened = Vec::new();
        for part in list_parts {
            if part.kind == crate::ListPartKind::Element {
                flattened.extend(
                    next_element
                        .next()
                        .expect("list formatter emitted more elements than duration units"),
                );
            } else {
                flattened.push(duration_literal(&part.value));
            }
        }
        debug_assert!(next_element.next().is_none());
        Ok(flattened)
    }
}

/// Returns requested locales currently backed by duration unit-pattern data.
pub fn supported_duration_format_locales(
    requested: &[crate::CanonicalLocale],
    matcher: crate::LocaleMatcher,
) -> Vec<crate::CanonicalLocale> {
    crate::supported_locales(crate::IntlService::DurationFormat, requested, matcher)
}

fn duration_supported_requests(
    requested: &[crate::CanonicalLocale],
    matcher: crate::LocaleMatcher,
) -> Vec<crate::CanonicalLocale> {
    vec![
        crate::resolve_locale(crate::IntlService::DurationFormat, requested, matcher)
            .selected()
            .clone(),
    ]
}

fn duration_list_style(style: DurationStyle) -> crate::ListStyle {
    match style {
        DurationStyle::Long => crate::ListStyle::Wide,
        DurationStyle::Short | DurationStyle::Digital => crate::ListStyle::Short,
        DurationStyle::Narrow => crate::ListStyle::Narrow,
    }
}

fn duration_literal(value: &str) -> DurationPart {
    DurationPart {
        kind: DurationPartKind::Literal,
        value: value.into(),
        unit: None,
    }
}

fn duration_should_display_sign(
    value: i128,
    duration_is_negative: bool,
    sign_displayed: &bool,
) -> bool {
    !*sign_displayed && (value < 0 || (value == 0 && duration_is_negative))
}

#[derive(Clone, Copy)]
enum FractionalDurationUnit {
    Seconds,
    Milliseconds,
    Microseconds,
}

impl FractionalDurationUnit {
    const fn exponent(self) -> u8 {
        match self {
            Self::Seconds => 9,
            Self::Milliseconds => 6,
            Self::Microseconds => 3,
        }
    }
}

fn duration_fractional_unit(unit: DurationUnit) -> Option<FractionalDurationUnit> {
    match unit {
        DurationUnit::Seconds => Some(FractionalDurationUnit::Seconds),
        DurationUnit::Milliseconds => Some(FractionalDurationUnit::Milliseconds),
        DurationUnit::Microseconds => Some(FractionalDurationUnit::Microseconds),
        _ => None,
    }
}

fn duration_next_unit_is_fractional(
    resolved: &ResolvedDurationFormatServiceOptions,
    unit: DurationUnit,
) -> bool {
    let next = match unit {
        DurationUnit::Seconds => Some(DurationUnit::Milliseconds),
        DurationUnit::Milliseconds => Some(DurationUnit::Microseconds),
        DurationUnit::Microseconds => Some(DurationUnit::Nanoseconds),
        _ => None,
    };
    next.is_some_and(|next| {
        duration_effective_style(next, resolved.unit_style(next))
            == DurationEffectiveUnitStyle::Fractional
    })
}

fn duration_fraction_digits(fraction: &str, exponent: u8, fixed: Option<u8>) -> String {
    let mut digits = fraction.to_owned();
    match fixed {
        Some(0) => return String::new(),
        Some(length) => {
            digits.truncate(usize::from(length));
            if digits.len() < usize::from(length) {
                digits.push_str(&"0".repeat(usize::from(length) - digits.len()));
            }
        }
        None => {
            digits.truncate(usize::from(exponent));
            let length = digits.trim_end_matches('0').len();
            digits.truncate(length);
        }
    }
    digits
}

fn duration_fraction_is_one(total: i128, exponent: u8) -> bool {
    total.unsigned_abs() == 10_u128.pow(exponent.into())
}

fn english_group_digits(digits: &str) -> String {
    let first = digits.len() % 3;
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    if first != 0 {
        grouped.push_str(&digits[..first]);
    }
    for (index, chunk) in digits.as_bytes()[first..].chunks(3).enumerate() {
        if !grouped.is_empty() || index != 0 {
            grouped.push(',');
        }
        grouped.push_str(std::str::from_utf8(chunk).expect("decimal digits are valid UTF-8"));
    }
    grouped
}

/// Resolves typed `Intl.DurationFormat` unit options using Edition 13 table
/// order and style compatibility rules.
pub fn resolve_duration_format_options(
    options: DurationFormatOptions,
) -> Result<ResolvedDurationFormatOptions, DurationFormatOptionsError> {
    resolve_duration_format_options_with_digital_format(options, false)
}

/// Resolves DurationFormat unit records with locale digital-pattern metadata.
///
/// This is kept crate-private because `two_digit_hours` is locale data, not a
/// user option. `DurationFormat::try_new` supplies the value selected by its
/// locale service; the public typed resolver uses the English/US default.
fn resolve_duration_format_options_with_digital_format(
    options: DurationFormatOptions,
    two_digit_hours: bool,
) -> Result<ResolvedDurationFormatOptions, DurationFormatOptionsError> {
    if options.fractional_digits.is_some_and(|digits| digits > 9) {
        return Err(DurationFormatOptionsError::FractionalDigitsOutOfRange);
    }
    let mut styles = [DurationUnitStyle::Short; 10];
    let mut displays = [DurationUnitDisplay::Auto; 10];
    let mut previous_style = None;
    for unit in DurationUnit::ALL {
        let index = unit.index();
        let unit_options = options.units[index];
        let mut display_default = DurationUnitDisplay::Always;
        let mut style = if let Some(style) = unit_options.style {
            style
        } else if options.style == DurationStyle::Digital {
            if !matches!(
                unit,
                DurationUnit::Hours | DurationUnit::Minutes | DurationUnit::Seconds
            ) {
                display_default = DurationUnitDisplay::Auto;
            }
            duration_default_style(options.style, unit)
        } else if matches!(
            previous_style,
            Some(
                DurationEffectiveUnitStyle::Fractional
                    | DurationEffectiveUnitStyle::Numeric
                    | DurationEffectiveUnitStyle::TwoDigit
            )
        ) {
            if !matches!(unit, DurationUnit::Minutes | DurationUnit::Seconds) {
                display_default = DurationUnitDisplay::Auto;
            }
            DurationUnitStyle::Numeric
        } else {
            display_default = DurationUnitDisplay::Auto;
            duration_default_style(options.style, unit)
        };
        if !duration_unit_allows_style(unit, style) {
            return Err(DurationFormatOptionsError::UnsupportedUnitStyle);
        }
        let effective_style = duration_effective_style(unit, style);
        if effective_style == DurationEffectiveUnitStyle::Fractional {
            display_default = DurationUnitDisplay::Auto;
        }
        let display = unit_options.display.unwrap_or(display_default);
        validate_duration_unit_style(effective_style, display, previous_style)?;
        if unit == DurationUnit::Hours && two_digit_hours {
            style = DurationUnitStyle::TwoDigit;
        }
        if matches!(unit, DurationUnit::Minutes | DurationUnit::Seconds)
            && matches!(
                previous_style,
                Some(DurationEffectiveUnitStyle::Numeric | DurationEffectiveUnitStyle::TwoDigit)
            )
        {
            style = DurationUnitStyle::TwoDigit;
        }
        styles[index] = style;
        displays[index] = display;
        if matches!(
            unit,
            DurationUnit::Hours
                | DurationUnit::Minutes
                | DurationUnit::Seconds
                | DurationUnit::Milliseconds
                | DurationUnit::Microseconds
        ) {
            previous_style = Some(duration_effective_style(unit, style));
        }
    }
    Ok(ResolvedDurationFormatOptions {
        style: options.style,
        unit_styles: styles,
        unit_displays: displays,
        fractional_digits: options.fractional_digits,
    })
}

fn duration_default_style(style: DurationStyle, unit: DurationUnit) -> DurationUnitStyle {
    match (style, unit) {
        (DurationStyle::Digital, DurationUnit::Hours) => DurationUnitStyle::Numeric,
        (DurationStyle::Digital, DurationUnit::Minutes | DurationUnit::Seconds) => {
            DurationUnitStyle::Numeric
        }
        (
            DurationStyle::Digital,
            DurationUnit::Milliseconds | DurationUnit::Microseconds | DurationUnit::Nanoseconds,
        ) => DurationUnitStyle::Numeric,
        _ => style.into(),
    }
}

/// The internal Duration Unit Options Record style. ECMA-402 represents
/// numeric millisecond, microsecond and nanosecond records as `fractional`
/// while `resolvedOptions()` reports their externally-visible `numeric` style.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DurationEffectiveUnitStyle {
    Fractional,
    Long,
    Short,
    Narrow,
    Numeric,
    TwoDigit,
}

fn duration_effective_style(
    unit: DurationUnit,
    style: DurationUnitStyle,
) -> DurationEffectiveUnitStyle {
    match (unit, style) {
        (
            DurationUnit::Milliseconds | DurationUnit::Microseconds | DurationUnit::Nanoseconds,
            DurationUnitStyle::Numeric,
        ) => DurationEffectiveUnitStyle::Fractional,
        (_, DurationUnitStyle::Long) => DurationEffectiveUnitStyle::Long,
        (_, DurationUnitStyle::Short) => DurationEffectiveUnitStyle::Short,
        (_, DurationUnitStyle::Narrow) => DurationEffectiveUnitStyle::Narrow,
        (_, DurationUnitStyle::Numeric) => DurationEffectiveUnitStyle::Numeric,
        (_, DurationUnitStyle::TwoDigit) => DurationEffectiveUnitStyle::TwoDigit,
    }
}

fn validate_duration_unit_style(
    style: DurationEffectiveUnitStyle,
    display: DurationUnitDisplay,
    previous_style: Option<DurationEffectiveUnitStyle>,
) -> Result<(), DurationFormatOptionsError> {
    if style == DurationEffectiveUnitStyle::Fractional && display == DurationUnitDisplay::Always {
        return Err(DurationFormatOptionsError::FractionalUnitAlwaysDisplay);
    }
    if previous_style == Some(DurationEffectiveUnitStyle::Fractional)
        && style != DurationEffectiveUnitStyle::Fractional
    {
        return Err(DurationFormatOptionsError::IncompatibleFractionalUnitStyles);
    }
    if matches!(
        previous_style,
        Some(DurationEffectiveUnitStyle::Numeric | DurationEffectiveUnitStyle::TwoDigit)
    ) && !matches!(
        style,
        DurationEffectiveUnitStyle::Fractional
            | DurationEffectiveUnitStyle::Numeric
            | DurationEffectiveUnitStyle::TwoDigit
    ) {
        return Err(DurationFormatOptionsError::IncompatibleUnitStyles);
    }
    Ok(())
}

fn duration_unit_allows_style(unit: DurationUnit, style: DurationUnitStyle) -> bool {
    match unit {
        DurationUnit::Years | DurationUnit::Months | DurationUnit::Weeks | DurationUnit::Days => {
            matches!(
                style,
                DurationUnitStyle::Long | DurationUnitStyle::Short | DurationUnitStyle::Narrow
            )
        }
        DurationUnit::Hours | DurationUnit::Minutes | DurationUnit::Seconds => true,
        DurationUnit::Milliseconds | DurationUnit::Microseconds | DurationUnit::Nanoseconds => {
            !matches!(style, DurationUnitStyle::TwoDigit)
        }
    }
}

impl DurationRecord {
    /// Constructs and validates an ECMA-402 Duration Record from integral
    /// mathematical values.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        years: i128,
        months: i128,
        weeks: i128,
        days: i128,
        hours: i128,
        minutes: i128,
        seconds: i128,
        milliseconds: i128,
        microseconds: i128,
        nanoseconds: i128,
    ) -> Result<Self, DurationRecordError> {
        let record = Self {
            years,
            months,
            weeks,
            days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
        };
        record.validate()?;
        Ok(record)
    }

    /// Converts fields already coerced with ECMAScript `ToNumber` into a
    /// validated Duration Record.
    ///
    /// The range is checked before conversion to retain the correct error for
    /// very large finite IEEE-754 inputs without relying on a saturating cast.
    #[allow(clippy::too_many_arguments)]
    pub fn try_from_f64(
        years: f64,
        months: f64,
        weeks: f64,
        days: f64,
        hours: f64,
        minutes: f64,
        seconds: f64,
        milliseconds: f64,
        microseconds: f64,
        nanoseconds: f64,
    ) -> Result<Self, DurationRecordError> {
        let values = [
            years,
            months,
            weeks,
            days,
            hours,
            minutes,
            seconds,
            milliseconds,
            microseconds,
            nanoseconds,
        ];
        let mut integral = [0_i128; 10];
        for (slot, value) in integral.iter_mut().zip(values) {
            if !value.is_finite() {
                return Err(DurationRecordError::NonFinite);
            }
            if value.fract() != 0.0 {
                return Err(DurationRecordError::NonIntegral);
            }
            // The largest field that can survive IsValidDuration is below
            // 2^83 nanoseconds, comfortably inside i128. This guard rejects
            // finite IEEE-754 values before Rust's float-to-int saturation.
            if value.abs() >= 2_f64.powi(100) {
                return Err(DurationRecordError::OutOfRange);
            }
            *slot = value as i128;
        }
        Self::try_new(
            integral[0],
            integral[1],
            integral[2],
            integral[3],
            integral[4],
            integral[5],
            integral[6],
            integral[7],
            integral[8],
            integral[9],
        )
    }

    /// Returns `-1`, `0`, or `1` according to `DurationSign`.
    pub fn sign(&self) -> i8 {
        self.values()
            .into_iter()
            .find_map(|value| (value != 0).then(|| value.signum() as i8))
            .unwrap_or(0)
    }

    /// Checks the Edition 13 `IsValidDuration` constraints.
    pub fn validate(&self) -> Result<(), DurationRecordError> {
        const MAX_CALENDAR_UNIT: i128 = 1_i128 << 32;
        const MAX_NORMALIZED_SECONDS: i128 = 1_i128 << 53;
        const NANOS_PER_SECOND: i128 = 1_000_000_000;

        if [self.years, self.months, self.weeks]
            .into_iter()
            .any(|value| value.unsigned_abs() >= MAX_CALENDAR_UNIT as u128)
        {
            return Err(DurationRecordError::OutOfRange);
        }

        let sign = self.sign();
        if self
            .values()
            .into_iter()
            .any(|value| value != 0 && value.signum() as i8 != sign)
        {
            return Err(DurationRecordError::MixedSign);
        }

        let normalized_nanoseconds = [
            self.days
                .checked_mul(86_400)
                .and_then(|value| value.checked_mul(NANOS_PER_SECOND)),
            self.hours
                .checked_mul(3_600)
                .and_then(|value| value.checked_mul(NANOS_PER_SECOND)),
            self.minutes
                .checked_mul(60)
                .and_then(|value| value.checked_mul(NANOS_PER_SECOND)),
            self.seconds.checked_mul(NANOS_PER_SECOND),
            self.milliseconds.checked_mul(1_000_000),
            self.microseconds.checked_mul(1_000),
            Some(self.nanoseconds),
        ]
        .into_iter()
        .try_fold(0_i128, |total, value| {
            total.checked_add(value.ok_or(())?).ok_or(())
        })
        .map_err(|()| DurationRecordError::OutOfRange)?;
        if normalized_nanoseconds.unsigned_abs()
            >= (MAX_NORMALIZED_SECONDS * NANOS_PER_SECOND) as u128
        {
            return Err(DurationRecordError::OutOfRange);
        }
        Ok(())
    }

    fn values(self) -> [i128; 10] {
        [
            self.years,
            self.months,
            self.weeks,
            self.days,
            self.hours,
            self.minutes,
            self.seconds,
            self.milliseconds,
            self.microseconds,
            self.nanoseconds,
        ]
    }

    fn value(self, unit: DurationUnit) -> i128 {
        self.values()[unit.index()]
    }

    /// Returns the exact mathematical value of a seconds-or-smaller unit,
    /// including every smaller duration field expressed at `unit` precision.
    /// Callers validate the record before invoking this helper, so the
    /// Edition 13 duration range proves these operations fit in `i128`.
    fn fractional_total(self, unit: FractionalDurationUnit) -> i128 {
        match unit {
            FractionalDurationUnit::Seconds => {
                self.seconds * 1_000_000_000
                    + self.milliseconds * 1_000_000
                    + self.microseconds * 1_000
                    + self.nanoseconds
            }
            FractionalDurationUnit::Milliseconds => {
                self.milliseconds * 1_000_000 + self.microseconds * 1_000 + self.nanoseconds
            }
            FractionalDurationUnit::Microseconds => self.microseconds * 1_000 + self.nanoseconds,
        }
    }
}

#[cfg(test)]
mod implementation_tests {
    use super::*;

    #[test]
    fn retains_locale_digital_metadata_and_total_pattern_fallbacks() {
        let resolved = resolve_duration_format_options_with_digital_format(
            DurationFormatOptions {
                style: DurationStyle::Digital,
                ..Default::default()
            },
            true,
        )
        .unwrap();
        assert_eq!(
            resolved.unit_style(DurationUnit::Hours),
            DurationUnitStyle::TwoDigit
        );

        // The service cannot route numeric styles here, but the total data
        // lookup remains defensive for a future host adapter.
        assert_eq!(
            crate::locale_data_provider().duration_unit_pattern(
                "en",
                DurationUnit::Seconds,
                DurationUnitStyle::Numeric,
                false,
            ),
            ("", "")
        );
    }
}
