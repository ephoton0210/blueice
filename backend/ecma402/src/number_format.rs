// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral implementation of the ECMA-402 number-format service.

use super::*;

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
    // Unit and currency-name selection are CLDR cardinal-plural operations
    // over the rounded decimal that is actually rendered. Keeping the shared
    // service here avoids reducing every locale to an English one/other
    // distinction while preserving the existing NumberFormat rounding path.
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
        let provider = NumberingSystemInspectionProvider::default();
        let mut formatter_options = DecimalFormatterOptions::default();
        formatter_options.grouping_strategy = Some(options.use_grouping.into());
        let mut preferences: DecimalFormatterPreferences = selected.locale().into();
        let selected_numbering_system = unicode_keyword(selected.locale(), "nu");
        if let Some(numbering_system) = selected_numbering_system.as_deref() {
            let value = numbering_system
                .parse::<icu_locale_core::extensions::unicode::Value>()
                .map_err(|_| NumberFormatError::DataUnavailable)?;
            preferences.numbering_system = Some(
                NumberingSystem::try_from(value).map_err(|_| NumberFormatError::DataUnavailable)?,
            );
        }
        let formatter =
            DecimalFormatter::try_new_unstable(&provider, preferences, formatter_options)
                .map_err(|_| NumberFormatError::DataUnavailable)?;
        let numbering_system = selected_numbering_system.unwrap_or_else(|| {
            provider
                .numbering_system
                .into_inner()
                .unwrap_or_else(|| "latn".into())
        });
        let decimal_digits = locale_data_provider()
            .decimal_digits(&numbering_system)
            .ok_or(NumberFormatError::DataUnavailable)?;
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
        let display_plural_rules = matches!(
            options.style,
            NumberFormatStyle::Unit | NumberFormatStyle::Currency
        )
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

        // CLDR range patterns commonly share a trailing currency, unit, or
        // percent affix. The formatted parts, rather than a language-family
        // table, tell us whether the selected provider pattern placed that
        // affix after the magnitude.
        let suffix_length = common_number_affix_suffix_length(&start_parts, &end_parts);
        // An exact-value compact pattern may consist solely of its `compact`
        // part (for example pinned French long `mille`). It is not an affix
        // when extracting it would erase an endpoint altogether.
        let suffix_is_affix = suffix_length > 0
            && suffix_length < start_parts.len()
            && suffix_length < end_parts.len();
        let suffix = if let Some(suffix) = hidden_unit_range_suffix {
            suffix
        } else if suffix_is_affix {
            let start_at = start_parts.len() - suffix_length;
            end_parts.truncate(end_parts.len() - suffix_length);
            start_parts.split_off(start_at)
        } else if let Some(suffix_length) =
            range_plural_affix_suffix_length(&start_parts, &end_parts)
        {
            // Rebuild the trailing affix with the CLDR plural-range category
            // over the rounded endpoint values. This normally matches the
            // end category, but preserves locales with an explicit range
            // rule as well as French `1–2 mètres`.
            let start_at = start_parts.len() - suffix_length;
            start_parts.truncate(start_at);
            let end_at = end_parts.len() - suffix_length;
            let end_suffix = end_parts.split_off(end_at);
            let range_plural_category =
                start_plural_category
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
        } else {
            Vec::new()
        };

        // The CLDR currency pattern folds a common explicit plus/currency
        // prefix into the start endpoint (for example `+$2.90–3.10`). A bare
        // prefix currency remains endpoint-specific (`$3 – $5`).
        let prefix_length = common_number_part_prefix_length(&start_parts, &end_parts);
        if prefix_length > 0
            && start_parts[..prefix_length]
                .iter()
                .any(|part| part.kind == NumberFormatPartKind::PlusSign)
            && (start_parts[..prefix_length]
                .iter()
                .any(|part| part.kind == NumberFormatPartKind::Currency)
                || suffix
                    .iter()
                    .any(|part| part.kind == NumberFormatPartKind::Currency))
        {
            end_parts.drain(..prefix_length);
        }

        let mut result = Vec::with_capacity(start_parts.len() + end_parts.len() + suffix.len() + 1);
        result.extend(start_parts.into_iter().map(start_number_range_part));
        result.push(NumberRangePart {
            kind: NumberFormatPartKind::Literal,
            value: number_range_separator(
                &self.resolved.locale,
                self.resolved.style,
                self.resolved.maximum_fraction_digits,
            )
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
        let display_plural_category = self
            .display_plural_rules
            .as_ref()
            .map(|rules| rules.select_decimal(&value.to_string()))
            .transpose()
            .map_err(|_| NumberFormatError::FormattingFailed)?;
        let compact = compact_scale.as_ref().and_then(|_| {
            crate::locale_data_provider().compact_number_pattern(
                &self.resolved.locale,
                compact_magnitude,
                self.compact_display,
                display_plural_category.unwrap_or(PluralCategory::Other),
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
            if !pattern.separator.is_empty() {
                collector.push(NumberFormatPartKind::Literal, &pattern.separator);
            }
            collector.push(NumberFormatPartKind::Compact, &pattern.suffix);
        }
        let numeric_parts = collector.parts.clone();
        let unit_hides_number = if let Some(currency) = self.currency.as_ref() {
            apply_currency_pattern(
                &mut collector.parts,
                currency,
                &self.resolved.locale,
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
            collector.push(NumberFormatPartKind::ExponentSeparator, "E");
            if exponent < 0 {
                collector.push(NumberFormatPartKind::ExponentMinusSign, "-");
            }
            let exponent = exponent.unsigned_abs().to_string();
            collector.push(NumberFormatPartKind::ExponentInteger, &exponent);
        }
        localize_decimal_parts(&mut collector.parts, &self.decimal_digits);
        let mut numeric_parts = numeric_parts;
        localize_decimal_parts(&mut numeric_parts, &self.decimal_digits);
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

fn number_sign_display_decimal(value: &str, display: NumberSignDisplay) -> &str {
    let magnitude = value.strip_prefix('-').unwrap_or(value);
    match display {
        NumberSignDisplay::Never => magnitude,
        NumberSignDisplay::Auto
        | NumberSignDisplay::Always
        | NumberSignDisplay::ExceptZero
        | NumberSignDisplay::Negative => value,
    }
}

fn number_sign_display_f64(value: f64, display: NumberSignDisplay) -> f64 {
    match display {
        NumberSignDisplay::Never => value.abs(),
        NumberSignDisplay::Auto
        | NumberSignDisplay::Always
        | NumberSignDisplay::ExceptZero
        | NumberSignDisplay::Negative => value,
    }
}

fn decimal_is_zero(value: &Decimal) -> bool {
    value
        .to_string()
        .trim_start_matches(['-', '+'])
        .bytes()
        .all(|byte| matches!(byte, b'0' | b'.'))
}

/// Maps ICU4X decimal writeable parts to ECMA-402 `formatToParts` records.
#[derive(Default)]
struct NumberPartCollector {
    parts: Vec<NumberFormatPart>,
    stack: Vec<NumberFormatPartKind>,
}

impl NumberPartCollector {
    fn push(&mut self, kind: NumberFormatPartKind, value: &str) {
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
fn localize_decimal_parts(parts: &mut [NumberFormatPart], digits: &[char; 10]) {
    for part in parts.iter_mut().filter(|part| {
        matches!(
            part.kind,
            NumberFormatPartKind::Integer
                | NumberFormatPartKind::Fraction
                | NumberFormatPartKind::ExponentInteger
        )
    }) {
        part.value = localize_decimal_digits(&part.value, digits);
    }
}

fn notation_exponent(value: &Decimal, notation: NumberNotation) -> Option<i16> {
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
fn compact_maximum_fraction_digits(value: &Decimal) -> u8 {
    let magnitude = value.nonzero_magnitude_start();
    u8::try_from((1 - magnitude).clamp(0, 100)).expect("clamped compact fraction digits")
}

/// Applies one ICU4X `DecimalDigitsV1` payload to ASCII decimal text.
///
/// DurationFormat's numeric substeps use the same helper so an accepted
/// `numberingSystem` affects both direct NumberFormat and duration output.
pub(crate) fn localize_decimal_digits(value: &str, digits: &[char; 10]) -> String {
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

fn apply_unit_pattern(
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
        return false;
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

fn resolve_fraction_digits(
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

fn resolve_significant_digits(
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
fn rounding_increment_parts(value: u16) -> Option<(RoundingIncrement, i16)> {
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

fn apply_currency_pattern(
    parts: &mut Vec<NumberFormatPart>,
    currency: &NumberCurrencyOptions,
    locale: &str,
    negative: bool,
    plural: PluralCategory,
) {
    let mut used_provider_pattern = false;
    if let Some(pattern) = crate::locale_data_provider().number_currency_pattern(
        locale,
        &currency.code,
        currency.display,
        currency.sign == NumberCurrencySign::Accounting
            && currency.display != NumberCurrencyDisplay::Name,
        negative,
        plural,
    ) {
        used_provider_pattern = true;
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
    } else {
        let symbol = currency_symbol(currency, locale);
        let currency_part = NumberFormatPart {
            kind: NumberFormatPartKind::Currency,
            value: symbol,
        };
        if uses_trailing_currency_pattern(locale) {
            parts.push(NumberFormatPart {
                kind: NumberFormatPartKind::Literal,
                value: "\u{a0}".into(),
            });
            parts.push(currency_part);
        } else {
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
            parts.insert(index, currency_part);
        }
    }
    let accounting_fallback = negative
        && currency.sign == NumberCurrencySign::Accounting
        && currency.display != NumberCurrencyDisplay::Name
        && !locale.starts_with("de");
    if accounting_fallback && !used_provider_pattern {
        parts.retain(|part| part.kind != NumberFormatPartKind::MinusSign);
        parts.insert(
            0,
            NumberFormatPart {
                kind: NumberFormatPartKind::Literal,
                value: "(".into(),
            },
        );
        parts.push(NumberFormatPart {
            kind: NumberFormatPartKind::Literal,
            value: ")".into(),
        });
    }
}

fn number_currency_pattern_part(piece: NumberCurrencyPatternPiece) -> NumberFormatPart {
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

fn apply_percent_pattern(parts: &mut Vec<NumberFormatPart>, locale: &str) {
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

    if uses_space_before_percent(locale) {
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

fn number_percent_pattern_part(
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

fn shared_number_range_part(part: NumberFormatPart) -> NumberRangePart {
    NumberRangePart {
        kind: part.kind,
        value: part.value,
        source: NumberRangePartSource::Shared,
    }
}

fn start_number_range_part(part: NumberFormatPart) -> NumberRangePart {
    NumberRangePart {
        kind: part.kind,
        value: part.value,
        source: NumberRangePartSource::StartRange,
    }
}

fn end_number_range_part(part: NumberFormatPart) -> NumberRangePart {
    NumberRangePart {
        kind: part.kind,
        value: part.value,
        source: NumberRangePartSource::EndRange,
    }
}

fn common_number_part_prefix_length(start: &[NumberFormatPart], end: &[NumberFormatPart]) -> usize {
    start
        .iter()
        .zip(end)
        .take_while(|(left, right)| left == right)
        .count()
}

fn common_number_affix_suffix_length(
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

/// Returns a structurally shared trailing unit or currency-name affix whose
/// localized label differs only because the two endpoints selected different
/// cardinal forms. The default CLDR plural-range category is the end
/// category, so the caller preserves this suffix from the end formatting.
fn range_plural_affix_suffix_length(
    start: &[NumberFormatPart],
    end: &[NumberFormatPart],
) -> Option<usize> {
    let (start_last, end_last) = (start.last()?, end.last()?);
    if start_last.kind != end_last.kind
        || !matches!(
            start_last.kind,
            NumberFormatPartKind::Unit | NumberFormatPartKind::Currency
        )
        || start_last.value == end_last.value
    {
        return None;
    }

    let mut length = 1;
    while length < start.len()
        && length < end.len()
        && start[start.len() - length - 1].kind == NumberFormatPartKind::Literal
        && start[start.len() - length - 1] == end[end.len() - length - 1]
    {
        length += 1;
    }
    (length < start.len() && length < end.len()).then_some(length)
}

/// A compact representation of the CLDR interval patterns needed by the
/// currently bundled number data. Decimal ranges normally use an en dash;
/// Portuguese's pattern uses a spaced hyphen, while fixed-zero-digit prefix
/// currencies use the spaced English pattern.
fn number_range_separator(
    locale: &str,
    style: NumberFormatStyle,
    maximum_fraction_digits: u8,
) -> &'static str {
    if locale.split('-').next().unwrap_or(locale) == "pt" {
        " - "
    } else if style == NumberFormatStyle::Currency && maximum_fraction_digits == 0 {
        " – "
    } else {
        "–"
    }
}

/// CLDR's common currency patterns place the symbol after the magnitude in
/// most European, Cyrillic, and right-to-left language families. The compact
/// decimal service intentionally carries this small pattern table instead of
/// pretending the English prefix is universal; ICU4X decimal symbols alone do
/// not expose currency-unit patterns.
fn uses_trailing_currency_pattern(locale: &str) -> bool {
    matches!(
        locale.split('-').next().unwrap_or(locale),
        "ar" | "be"
            | "bg"
            | "ca"
            | "cs"
            | "da"
            | "de"
            | "el"
            | "es"
            | "et"
            | "fi"
            | "fr"
            | "he"
            | "hr"
            | "hu"
            | "is"
            | "it"
            | "lt"
            | "lv"
            | "nl"
            | "no"
            | "pl"
            | "pt"
            | "ro"
            | "ru"
            | "sk"
            | "sl"
            | "sr"
            | "sv"
            | "tr"
            | "uk"
    )
}

fn uses_space_before_percent(locale: &str) -> bool {
    uses_trailing_currency_pattern(locale)
        && !matches!(
            locale.split('-').next().unwrap_or(locale),
            "ar" | "he" | "tr"
        )
}

fn currency_symbol(currency: &NumberCurrencyOptions, locale: &str) -> String {
    match currency.display {
        NumberCurrencyDisplay::Code => currency.code.clone(),
        NumberCurrencyDisplay::Name => match currency.code.as_str() {
            "USD" => "US dollars".into(),
            "EUR" => "euros".into(),
            _ => currency.code.clone(),
        },
        NumberCurrencyDisplay::Symbol | NumberCurrencyDisplay::NarrowSymbol => {
            crate::locale_data_provider()
                .currency_symbol(locale, &currency.code, currency.display)
                .unwrap_or_else(|| currency.code.clone())
        }
    }
}

/// Captures the exact numbering system selected by ICU4X during construction.
///
/// `DecimalFormatter` deliberately hides its data-provider internals. The
/// provider protocol nevertheless defines the final `DecimalDigitsV1` request
/// as the resolved numbering system, so this transparent wrapper makes the
/// resolved ECMA-402 value observable without exposing ICU4X types to callers.
#[derive(Default)]
struct NumberingSystemInspectionProvider {
    numbering_system: RefCell<Option<String>>,
}

impl<M> DataProvider<M> for NumberingSystemInspectionProvider
where
    M: DataMarker,
    DecimalData: DataProvider<M>,
{
    fn load(
        &self,
        request: DataRequest,
    ) -> Result<icu_provider::DataResponse<M>, icu_provider::DataError> {
        if TypeId::of::<M>() == TypeId::of::<DecimalDigitsV1>() {
            self.numbering_system
                .replace(Some(request.id.marker_attributes.as_str().into()));
        }
        DecimalData.load(request)
    }
}
