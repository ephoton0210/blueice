// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral implementation of the ECMA-402 number-format service.

use super::*;
use icu_decimal::provider::{
    DecimalDigitsV1, DecimalSymbolStrsBuilder, DecimalSymbols, DecimalSymbolsV1, GroupingSizes,
};
use icu_provider::{DataPayload, DataResponse};
use zerovec::VarZeroCow;

/// Whether the shared provider has decimal data for this locale.
pub fn supports_number_format_locale(locale: &IcuLocale) -> bool {
    locale_data_provider().supports_service_locale(IntlService::NumberFormat, locale)
}

/// One canonical locale considered during decimal number-format negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberFormatLocaleCandidate {
    requested: CanonicalLocale,
    supported: bool,
}

impl NumberFormatLocaleCandidate {
    /// Returns the canonical locale supplied by the host.
    pub fn requested(&self) -> &CanonicalLocale {
        &self.requested
    }

    /// Returns whether the bundled decimal data supports this request.
    pub fn is_supported(&self) -> bool {
        self.supported
    }
}

/// A deterministic trace of decimal number-format locale negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberFormatLocaleNegotiation {
    matcher: LocaleMatcher,
    candidates: Vec<NumberFormatLocaleCandidate>,
    selected: CanonicalLocale,
    used_default: bool,
}

impl NumberFormatLocaleNegotiation {
    /// Returns the matcher used for this negotiation.
    pub fn matcher(&self) -> LocaleMatcher {
        self.matcher
    }

    /// Returns every requested locale and its data-support decision.
    pub fn candidates(&self) -> &[NumberFormatLocaleCandidate] {
        &self.candidates
    }

    /// Returns the locale selected for decimal formatting.
    pub fn selected(&self) -> &CanonicalLocale {
        &self.selected
    }

    /// Returns whether the stable `en-US` service default was selected.
    pub fn used_default(&self) -> bool {
        self.used_default
    }
}

/// Negotiates a requested locale list against bundled decimal-format data.
///
/// Region and script subtags remain attached to a supported request so ICU4X
/// can select CLDR data. Unicode extensions are likewise retained for the
/// formatter's option resolution. If no requested locale is supported, the
/// stable service default is `en-US`.
pub fn negotiate_number_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> NumberFormatLocaleNegotiation {
    let resolution = resolve_locale(IntlService::NumberFormat, requested, matcher);
    NumberFormatLocaleNegotiation {
        matcher: resolution.matcher(),
        candidates: resolution
            .candidates()
            .iter()
            .map(|candidate| NumberFormatLocaleCandidate {
                requested: candidate.requested().clone(),
                supported: candidate.is_supported(),
            })
            .collect(),
        selected: resolution.selected().clone(),
        used_default: resolution.used_default(),
    }
}

/// Resolves one requested locale against bundled decimal-format data.
pub fn resolve_number_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    negotiate_number_format_locale(requested, matcher)
        .selected
        .clone()
}

/// Returns the requested locales supported by the bundled decimal service.
pub fn supported_number_format_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    supported_locales(IntlService::NumberFormat, requested, matcher)
}

/// The decimal grouping policy selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberGrouping {
    /// Apply the locale's ordinary grouping policy.
    #[default]
    Auto,
    /// Suppress grouping separators.
    Never,
    /// Apply grouping whenever the locale data permits it.
    Always,
    /// Group only when at least two digits precede the last grouping separator.
    Min2,
}

impl From<NumberGrouping> for GroupingStrategy {
    fn from(value: NumberGrouping) -> Self {
        match value {
            NumberGrouping::Auto => Self::Auto,
            NumberGrouping::Never => Self::Never,
            NumberGrouping::Always => Self::Always,
            NumberGrouping::Min2 => Self::Min2,
        }
    }
}

/// The `style` option selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberFormatStyle {
    /// A locale-sensitive decimal number.
    #[default]
    Decimal,
    /// A number multiplied by one hundred and rendered with a percent pattern.
    Percent,
    /// A number with a currency pattern.
    Currency,
    /// A number with a measurement-unit pattern.
    Unit,
}

/// The `notation` option selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberNotation {
    /// Ordinary decimal notation.
    #[default]
    Standard,
    /// Scientific notation with a single leading integer digit.
    Scientific,
    /// Scientific notation whose exponent is divisible by three.
    Engineering,
    /// Locale-specific compact notation.
    Compact,
}

/// The `compactDisplay` option selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberCompactDisplay {
    /// The locale's abbreviated compact pattern.
    #[default]
    Short,
    /// The locale's spelled-out compact pattern.
    Long,
}

/// The `currencyDisplay` option selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberCurrencyDisplay {
    /// Display the locale's ordinary currency symbol.
    #[default]
    Symbol,
    /// Display the ISO 4217 code.
    Code,
    /// Display the localized currency name.
    Name,
    /// Display the locale's narrow currency symbol.
    NarrowSymbol,
}

/// The `currencySign` option selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberCurrencySign {
    /// Render negative values with the locale's standard sign pattern.
    #[default]
    Standard,
    /// Use an accounting pattern when the locale supplies one.
    Accounting,
}

/// Host-neutral currency data resolved for one number formatter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberCurrencyOptions {
    /// The canonical uppercase ISO 4217 currency code.
    pub code: String,
    /// The requested currency display width.
    pub display: NumberCurrencyDisplay,
    /// The requested negative currency-sign pattern.
    pub sign: NumberCurrencySign,
}

/// Supplemental NumberFormat policies resolved by an embedding's option
/// adapter. Keeping these in one typed record makes a data-only
/// `numberingSystem` preference available even when locale negotiation uses
/// the default locale.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberFormatConstructionOptions {
    /// The selected ECMA-402 rounding increment.
    pub rounding_increment: u16,
    /// The optional lower significant-digit bound.
    pub minimum_significant_digits: Option<u8>,
    /// The optional upper significant-digit bound.
    pub maximum_significant_digits: Option<u8>,
    /// Whether an integral formatted value sheds trailing zeros.
    pub trailing_zero_display: NumberTrailingZeroDisplay,
    /// The currency record selected by the embedding.
    pub currency: Option<NumberCurrencyOptions>,
    /// An explicit, syntactically valid `numberingSystem` option.
    pub numbering_system: Option<String>,
}

/// ECMA-402 sanctioned units.
///
/// [`Self::ALL`] deliberately contains only simple-unit identifiers, as
/// required by `Intl.supportedValuesOf("unit")`. A `-per-` compound carries
/// two indexes into that canonical inventory and remains `Copy`, allowing the
/// resolved option record to keep its existing value semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberFormatUnit {
    /// Acres.
    Acre,
    /// Bits.
    Bit,
    /// Bytes.
    Byte,
    /// Degrees Celsius.
    Celsius,
    /// Centimeters.
    Centimeter,
    /// Days.
    Day,
    /// Angular degrees.
    Degree,
    /// Degrees Fahrenheit.
    Fahrenheit,
    /// Fluid ounces.
    FluidOunce,
    /// Feet.
    Foot,
    /// Gallons.
    Gallon,
    /// Gigabits.
    Gigabit,
    /// Gigabytes.
    Gigabyte,
    /// Grams.
    Gram,
    /// Hectares.
    Hectare,
    /// Hours.
    Hour,
    /// Inches.
    Inch,
    /// Kilobits.
    Kilobit,
    /// Kilobytes.
    Kilobyte,
    /// Kilograms.
    Kilogram,
    /// Kilometers.
    Kilometer,
    /// Liters.
    Liter,
    /// Megabits.
    Megabit,
    /// Megabytes.
    Megabyte,
    /// Meters.
    Meter,
    /// Microseconds.
    Microsecond,
    /// Miles.
    Mile,
    /// Scandinavian miles.
    MileScandinavian,
    /// Milliliters.
    Milliliter,
    /// Millimeters.
    Millimeter,
    /// Milliseconds.
    Millisecond,
    /// Minutes.
    Minute,
    /// Calendar months.
    Month,
    /// Nanoseconds.
    Nanosecond,
    /// Ounces.
    Ounce,
    /// A percentage unit.
    Percent,
    /// Petabytes.
    Petabyte,
    /// Pounds.
    Pound,
    /// Seconds.
    Second,
    /// Stones.
    Stone,
    /// Terabits.
    Terabit,
    /// Terabytes.
    Terabyte,
    /// Seven-day weeks.
    Week,
    /// Yards.
    Yard,
    /// Calendar years.
    Year,
    /// A sanctioned numerator/denominator compound such as
    /// `kilometer-per-hour`.
    CompoundPer {
        /// The numerator's index in [`Self::ALL`].
        numerator: u8,
        /// The denominator's index in [`Self::ALL`].
        denominator: u8,
    },
}

impl NumberFormatUnit {
    /// All sanctioned simple unit identifiers, in ECMA-402 canonical order.
    pub const ALL: &[Self] = &[
        Self::Acre,
        Self::Bit,
        Self::Byte,
        Self::Celsius,
        Self::Centimeter,
        Self::Day,
        Self::Degree,
        Self::Fahrenheit,
        Self::FluidOunce,
        Self::Foot,
        Self::Gallon,
        Self::Gigabit,
        Self::Gigabyte,
        Self::Gram,
        Self::Hectare,
        Self::Hour,
        Self::Inch,
        Self::Kilobit,
        Self::Kilobyte,
        Self::Kilogram,
        Self::Kilometer,
        Self::Liter,
        Self::Megabit,
        Self::Megabyte,
        Self::Meter,
        Self::Microsecond,
        Self::Mile,
        Self::MileScandinavian,
        Self::Milliliter,
        Self::Millimeter,
        Self::Millisecond,
        Self::Minute,
        Self::Month,
        Self::Nanosecond,
        Self::Ounce,
        Self::Percent,
        Self::Petabyte,
        Self::Pound,
        Self::Second,
        Self::Stone,
        Self::Terabit,
        Self::Terabyte,
        Self::Week,
        Self::Yard,
        Self::Year,
    ];

    /// Parses a sanctioned simple or `-per-` compound unit identifier.
    pub fn parse(value: &str) -> Option<Self> {
        if let Some(simple) = Self::parse_simple(value) {
            return Some(simple);
        }
        let (numerator, denominator) = value.split_once("-per-")?;
        if numerator.is_empty() || denominator.is_empty() || denominator.contains("-per-") {
            return None;
        }
        Some(Self::CompoundPer {
            numerator: Self::simple_index(numerator)?,
            denominator: Self::simple_index(denominator)?,
        })
    }

    fn parse_simple(value: &str) -> Option<Self> {
        match value {
            "acre" => Some(Self::Acre),
            "bit" => Some(Self::Bit),
            "byte" => Some(Self::Byte),
            "celsius" => Some(Self::Celsius),
            "centimeter" => Some(Self::Centimeter),
            "day" => Some(Self::Day),
            "degree" => Some(Self::Degree),
            "fahrenheit" => Some(Self::Fahrenheit),
            "fluid-ounce" => Some(Self::FluidOunce),
            "foot" => Some(Self::Foot),
            "gallon" => Some(Self::Gallon),
            "gigabit" => Some(Self::Gigabit),
            "gigabyte" => Some(Self::Gigabyte),
            "gram" => Some(Self::Gram),
            "hectare" => Some(Self::Hectare),
            "hour" => Some(Self::Hour),
            "inch" => Some(Self::Inch),
            "kilobit" => Some(Self::Kilobit),
            "kilobyte" => Some(Self::Kilobyte),
            "kilogram" => Some(Self::Kilogram),
            "kilometer" => Some(Self::Kilometer),
            "liter" => Some(Self::Liter),
            "megabit" => Some(Self::Megabit),
            "megabyte" => Some(Self::Megabyte),
            "meter" => Some(Self::Meter),
            "minute" => Some(Self::Minute),
            "month" => Some(Self::Month),
            "millisecond" => Some(Self::Millisecond),
            "microsecond" => Some(Self::Microsecond),
            "mile" => Some(Self::Mile),
            "mile-scandinavian" => Some(Self::MileScandinavian),
            "milliliter" => Some(Self::Milliliter),
            "millimeter" => Some(Self::Millimeter),
            "nanosecond" => Some(Self::Nanosecond),
            "ounce" => Some(Self::Ounce),
            "percent" => Some(Self::Percent),
            "petabyte" => Some(Self::Petabyte),
            "pound" => Some(Self::Pound),
            "second" => Some(Self::Second),
            "stone" => Some(Self::Stone),
            "terabit" => Some(Self::Terabit),
            "terabyte" => Some(Self::Terabyte),
            "week" => Some(Self::Week),
            "yard" => Some(Self::Yard),
            "year" => Some(Self::Year),
            _ => None,
        }
    }

    fn simple_index(value: &str) -> Option<u8> {
        Self::ALL
            .iter()
            .position(|unit| unit.as_str() == value)
            .and_then(|index| u8::try_from(index).ok())
    }

    fn simple_at(index: u8) -> Self {
        Self::ALL
            .get(usize::from(index))
            .copied()
            .expect("compound unit indexes are derived from NumberFormatUnit::ALL")
    }

    /// Returns the simple numerator and denominator for a compound unit.
    pub fn compound_parts(self) -> Option<(Self, Self)> {
        let Self::CompoundPer {
            numerator,
            denominator,
        } = self
        else {
            return None;
        };
        Some((Self::simple_at(numerator), Self::simple_at(denominator)))
    }

    /// Returns whether this is a compound rather than a simple unit.
    pub const fn is_compound(self) -> bool {
        matches!(self, Self::CompoundPer { .. })
    }

    /// Returns a simple-unit identifier, or a stable placeholder for internal
    /// callers which require a borrowed value. Use [`Self::identifier`] for a
    /// resolved compound identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Acre => "acre",
            Self::Bit => "bit",
            Self::Byte => "byte",
            Self::Celsius => "celsius",
            Self::Centimeter => "centimeter",
            Self::Day => "day",
            Self::Degree => "degree",
            Self::Fahrenheit => "fahrenheit",
            Self::FluidOunce => "fluid-ounce",
            Self::Foot => "foot",
            Self::Gallon => "gallon",
            Self::Gigabit => "gigabit",
            Self::Gigabyte => "gigabyte",
            Self::Gram => "gram",
            Self::Hectare => "hectare",
            Self::Hour => "hour",
            Self::Inch => "inch",
            Self::Kilobit => "kilobit",
            Self::Kilobyte => "kilobyte",
            Self::Kilogram => "kilogram",
            Self::Kilometer => "kilometer",
            Self::Liter => "liter",
            Self::Megabit => "megabit",
            Self::Megabyte => "megabyte",
            Self::Meter => "meter",
            Self::Minute => "minute",
            Self::Month => "month",
            Self::Millisecond => "millisecond",
            Self::Microsecond => "microsecond",
            Self::Mile => "mile",
            Self::MileScandinavian => "mile-scandinavian",
            Self::Milliliter => "milliliter",
            Self::Millimeter => "millimeter",
            Self::Nanosecond => "nanosecond",
            Self::Ounce => "ounce",
            Self::Percent => "percent",
            Self::Petabyte => "petabyte",
            Self::Pound => "pound",
            Self::Second => "second",
            Self::Stone => "stone",
            Self::Terabit => "terabit",
            Self::Terabyte => "terabyte",
            Self::Week => "week",
            Self::Yard => "yard",
            Self::Year => "year",
            Self::CompoundPer { .. } => "compound",
        }
    }

    /// Returns the canonical ECMA-402 identifier, including `-per-` compound
    /// identifiers.
    pub fn identifier(self) -> String {
        match self.compound_parts() {
            Some((numerator, denominator)) => {
                format!("{}-per-{}", numerator.as_str(), denominator.as_str())
            }
            None => self.as_str().into(),
        }
    }
}

/// The `unitDisplay` option selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberUnitDisplay {
    /// The CLDR short unit pattern.
    #[default]
    Short,
    /// The CLDR narrow unit pattern.
    Narrow,
    /// The CLDR long unit pattern.
    Long,
}

/// The `signDisplay` options required by number and duration formatting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberSignDisplay {
    /// Render a negative sign, including for negative zero.
    #[default]
    Auto,
    /// Suppress all signs.
    Never,
    /// Render a sign for every number, including both zeroes.
    Always,
    /// Render a sign for non-zero numbers only.
    ExceptZero,
    /// Render a negative sign for negative non-zero numbers only.
    Negative,
}

/// The `roundingMode` option selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberRoundingMode {
    /// Round halfway values away from zero.
    #[default]
    HalfExpand,
    /// Round toward negative infinity.
    Floor,
    /// Round toward positive infinity.
    Ceil,
    /// Round away from zero.
    Expand,
    /// Round toward zero.
    Trunc,
    /// Round halfway values toward positive infinity.
    HalfCeil,
    /// Round halfway values toward negative infinity.
    HalfFloor,
    /// Round halfway values toward zero.
    HalfTrunc,
    /// Round halfway values to even.
    HalfEven,
}

/// The precision family selected when both fraction and significant digit
/// options are present.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberRoundingPriority {
    /// Prefer significant digits when they were requested.
    #[default]
    Auto,
    /// Select the candidate with the smaller rounding magnitude.
    MorePrecision,
    /// Select the candidate with the larger rounding magnitude.
    LessPrecision,
}

/// The `trailingZeroDisplay` policy selected by `Intl.NumberFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NumberTrailingZeroDisplay {
    /// Preserve the resolved minimum fraction/significant-digit precision.
    #[default]
    Auto,
    /// Remove the fractional representation when its rounded value is integral.
    StripIfInteger,
}

impl NumberRoundingMode {
    fn fixed_decimal_mode(self) -> SignedRoundingMode {
        match self {
            Self::HalfExpand => SignedRoundingMode::Unsigned(UnsignedRoundingMode::HalfExpand),
            Self::Floor => SignedRoundingMode::Floor,
            Self::Ceil => SignedRoundingMode::Ceil,
            Self::Expand => SignedRoundingMode::Unsigned(UnsignedRoundingMode::Expand),
            Self::Trunc => SignedRoundingMode::Unsigned(UnsignedRoundingMode::Trunc),
            Self::HalfCeil => SignedRoundingMode::HalfCeil,
            Self::HalfFloor => SignedRoundingMode::HalfFloor,
            Self::HalfTrunc => SignedRoundingMode::Unsigned(UnsignedRoundingMode::HalfTrunc),
            Self::HalfEven => SignedRoundingMode::Unsigned(UnsignedRoundingMode::HalfEven),
        }
    }
}

/// Host-neutral options for the finite-decimal, currency, and duration-unit
/// subset of `Intl.NumberFormat`.
///
/// Percent, compact/scientific notation, and ranges are separately tracked
/// service slices. The fields here are enough to execute the standard's
/// DurationFormat delegation through NumberFormat.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NumberFormatOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// When locale-specific grouping separators are rendered.
    pub use_grouping: NumberGrouping,
    /// The selected number/unit pattern family.
    pub style: NumberFormatStyle,
    /// The selected numeric notation.
    pub notation: NumberNotation,
    /// The selected compact-pattern width.
    pub compact_display: NumberCompactDisplay,
    /// The measurement unit required when `style` is [`NumberFormatStyle::Unit`].
    pub unit: Option<NumberFormatUnit>,
    /// The unit-pattern width.
    pub unit_display: NumberUnitDisplay,
    /// The minimum number of integer digits, from one through 21.
    pub minimum_integer_digits: u8,
    /// The minimum number of fractional digits after rounding.
    pub minimum_fraction_digits: Option<u8>,
    /// The maximum number of fractional digits after rounding.
    pub maximum_fraction_digits: Option<u8>,
    /// The rounding algorithm for finite decimal values.
    pub rounding_mode: NumberRoundingMode,
    /// How fraction and significant digit precisions interact.
    pub rounding_priority: NumberRoundingPriority,
    /// Whether a negative sign is emitted.
    pub sign_display: NumberSignDisplay,
}

impl Default for NumberFormatOptions {
    fn default() -> Self {
        Self {
            locale_matcher: LocaleMatcher::default(),
            use_grouping: NumberGrouping::default(),
            style: NumberFormatStyle::default(),
            notation: NumberNotation::default(),
            compact_display: NumberCompactDisplay::default(),
            unit: None,
            unit_display: NumberUnitDisplay::default(),
            minimum_integer_digits: 1,
            minimum_fraction_digits: None,
            maximum_fraction_digits: None,
            rounding_mode: NumberRoundingMode::default(),
            rounding_priority: NumberRoundingPriority::default(),
            sign_display: NumberSignDisplay::default(),
        }
    }
}

/// ECMAScript-observable data resolved by the decimal number formatter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedNumberFormatOptions {
    /// The negotiated locale, including its supported Unicode extensions.
    pub locale: String,
    /// The actual ICU4X decimal numbering system.
    pub numbering_system: String,
    /// The selected grouping policy.
    pub use_grouping: NumberGrouping,
    /// The selected number/unit pattern family.
    pub style: NumberFormatStyle,
    /// The selected numeric notation.
    pub notation: NumberNotation,
    /// The resolved unit when the style is `unit`.
    pub unit: Option<NumberFormatUnit>,
    /// The resolved unit-pattern width.
    pub unit_display: NumberUnitDisplay,
    /// The resolved minimum integer digit count.
    pub minimum_integer_digits: u8,
    /// The resolved minimum number of fraction digits.
    pub minimum_fraction_digits: u8,
    /// The resolved maximum number of fraction digits.
    pub maximum_fraction_digits: u8,
    /// The resolved rounding algorithm.
    pub rounding_mode: NumberRoundingMode,
    /// The resolved sign policy.
    pub sign_display: NumberSignDisplay,
}

/// A single `Intl.NumberFormat.prototype.formatToParts` result record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberFormatPart {
    /// The ECMA-402 part type.
    pub kind: NumberFormatPartKind,
    /// The localized text for this contiguous part.
    pub value: String,
}

/// An already-converted input to the host-neutral number formatter.
///
/// JavaScript's `ToIntlMathematicalValue` preserves decimal strings and
/// BigInts instead of first rounding them through IEEE-754. Embedders use the
/// decimal variant for those exact values and the number variant for ordinary
/// ECMAScript Numbers (including `NaN` and infinities).
#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormatInput {
    /// A finite, base-10 decimal value whose spelling must remain exact.
    Decimal(String),
    /// A finite base-10 significand and exponent whose value must remain
    /// exact. This is the scientific spelling accepted by ECMA-402's
    /// `StringIntlMV`, rather than the formatter's output notation.
    ScientificDecimal {
        /// The signed base-10 significand, without an exponent.
        significand: String,
        /// The base-10 exponent to apply to `significand`.
        exponent: i16,
    },
    /// An IEEE-754 Number value.
    Number(f64),
}

/// Which end of a formatted number range supplied a part.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberRangePartSource {
    /// Text shared by both range endpoints.
    Shared,
    /// Text supplied by the range start.
    StartRange,
    /// Text supplied by the range end.
    EndRange,
}

/// One `Intl.NumberFormat.prototype.formatRangeToParts` result record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberRangePart {
    /// The ECMA-402 part type.
    pub kind: NumberFormatPartKind,
    /// The localized text for this contiguous part.
    pub value: String,
    /// The endpoint (or common range pattern) that supplied this part.
    pub source: NumberRangePartSource,
}

/// The ECMA-402 type of one number-format part.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberFormatPartKind {
    /// The negative sign.
    MinusSign,
    /// The positive sign.
    PlusSign,
    /// The approximation marker used when both endpoints round alike.
    ApproximatelySign,
    /// The scientific-notation exponent separator.
    ExponentSeparator,
    /// The negative sign of a scientific-notation exponent.
    ExponentMinusSign,
    /// The exponent digits of scientific notation.
    ExponentInteger,
    /// A contiguous integer digit sequence.
    Integer,
    /// A grouping separator.
    Group,
    /// A decimal separator.
    Decimal,
    /// A contiguous fractional digit sequence.
    Fraction,
    /// Locale or unit-pattern text outside numeric fields.
    Literal,
    /// The localized measurement-unit label.
    Unit,
    /// The localized currency symbol, code, or name.
    Currency,
    /// The localized percent sign.
    PercentSign,
    /// The locale-selected compact-notation suffix.
    Compact,
    /// A localized not-a-number symbol.
    Nan,
    /// A localized infinity symbol.
    Infinity,
}

/// A failure while constructing or using a decimal number formatter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberFormatError {
    /// The selected decimal data was unavailable.
    DataUnavailable,
    /// A fraction-digit option exceeded ECMA-402's supported range of 0–100.
    FractionDigitsOutOfRange,
    /// The requested minimum fraction digits exceeded the maximum.
    IncompatibleFractionDigits,
    /// The `roundingIncrement` value was not one of ECMA-402's permitted values.
    InvalidRoundingIncrement,
    /// A non-default `roundingIncrement` requires matching fraction digits.
    IncompatibleRoundingIncrement,
    /// A significant-digit option was outside ECMA-402's range of 1–21.
    SignificantDigitsOutOfRange,
    /// The requested minimum significant digits exceeded the maximum.
    IncompatibleSignificantDigits,
    /// `style: "currency"` had no ISO 4217 code.
    MissingCurrency,
    /// `style: "unit"` had no unit identifier.
    MissingUnit,
    /// `minimumIntegerDigits` was outside the range 1–21.
    MinimumIntegerDigitsOutOfRange,
    /// A decimal input was not a finite, base-10 decimal string.
    InvalidDecimal,
    /// An IEEE-754 input was `NaN` or infinite.
    NonFiniteNumber,
    /// A range endpoint became `NaN` during `ToIntlMathematicalValue`.
    RangeNaN,
    /// A writeable formatter failed while producing parts.
    FormattingFailed,
}

impl std::fmt::Display for NumberFormatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("decimal data is unavailable"),
            Self::FractionDigitsOutOfRange => {
                formatter.write_str("fraction digits must be in the range 0 through 100")
            }
            Self::IncompatibleFractionDigits => {
                formatter.write_str("minimum fraction digits exceed maximum fraction digits")
            }
            Self::InvalidRoundingIncrement => formatter.write_str("invalid rounding increment"),
            Self::IncompatibleRoundingIncrement => {
                formatter.write_str("rounding increment requires matching fraction digits")
            }
            Self::SignificantDigitsOutOfRange => {
                formatter.write_str("significant digits must be in the range 1 through 21")
            }
            Self::IncompatibleSignificantDigits => {
                formatter.write_str("minimum significant digits exceed maximum significant digits")
            }
            Self::MissingCurrency => formatter.write_str("currency style requires a currency code"),
            Self::MissingUnit => formatter.write_str("unit style requires a unit"),
            Self::MinimumIntegerDigitsOutOfRange => {
                formatter.write_str("minimum integer digits must be in the range 1 through 21")
            }
            Self::InvalidDecimal => formatter.write_str("invalid finite decimal input"),
            Self::NonFiniteNumber => formatter.write_str("number must be finite"),
            Self::RangeNaN => formatter.write_str("number range endpoints must not be NaN"),
            Self::FormattingFailed => formatter.write_str("number formatting failed"),
        }
    }
}

impl std::error::Error for NumberFormatError {}

/// A host-neutral, locale-sensitive decimal formatter.
///
/// Embedders retain ECMAScript coercion, `NaN`/infinity handling and object
/// semantics. Finite decimal formatting, option resolution and all ICU4X data
/// selection occur here.
pub struct NumberFormat {
    formatter: DecimalFormatter,
    // ICU4X's DecimalFormatter owns this mapping internally, but the
    // handwritten scientific and range-part paths build numeric text before
    // it reaches that formatter. Retain the exact `DecimalDigitsV1` payload
    // selected at construction so those parts use provider data too.
    decimal_digits: [char; 10],
    // ICU4X's decimal payload does not currently expose CLDR's scientific
    // `exponential` symbol. The shared provider keeps that typed record with
    // the exponent-minus bidi/sign shape selected for this formatter.
    scientific_symbols: crate::locale_data::NumberScientificSymbols,
    // Unit/currency-name selection and compact notation use CLDR plural
    // operations over the rounded decimal. Compact additionally carries the
    // provider-selected exponent into its plural operands.
    display_plural_rules: Option<PluralRules>,
    negotiation: NumberFormatLocaleNegotiation,
    resolved: ResolvedNumberFormatOptions,
    rounding_increment: u16,
    significant_digits: Option<(u8, u8)>,
    rounding_priority: NumberRoundingPriority,
    trailing_zero_display: NumberTrailingZeroDisplay,
    compact_display: NumberCompactDisplay,
    currency: Option<NumberCurrencyOptions>,
}

/// The internal output of one rounded NumberFormat operation.
///
/// The plural category is retained for `formatRange`: a unit or currency name
/// uses CLDR plural-range data over the categories of the decimals actually
/// rendered, not the source values supplied by the embedding.
struct FormattedNumber {
    parts: Vec<NumberFormatPart>,
    numeric_parts: Vec<NumberFormatPart>,
    display_plural_category: Option<PluralCategory>,
    unit_hides_number: bool,
}

mod formatting;
mod implementation;

use formatting::*;

pub(crate) fn localize_decimal_digits(value: &str, digits: &[char; 10]) -> String {
    formatting::localize_decimal_digits_impl(value, digits)
}
