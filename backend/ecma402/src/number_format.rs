// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral implementation of the ECMA-402 number-format service.

use super::*;

/// Whether the bundled decimal data has a locale-specific fallback for a
/// locale.
///
/// ICU4X falls all unknown languages back to `und`; that fallback is useful
/// for internal data loading but is not an ECMA-402 available-locale match.
/// A language is therefore supported when the decimal-symbol data resolves to
/// the same primary language (possibly after dropping region or script).
pub fn supports_number_format_locale(locale: &IcuLocale) -> bool {
    if supports_locale_language(locale) {
        return true;
    }
    let requested = icu_provider::DataLocale::from(locale);
    let response = <DecimalData as DataProvider<DecimalSymbolsV1>>::load(
        &DecimalData,
        DataRequest {
            id: DataIdentifierBorrowed::for_locale(&requested),
            metadata: Default::default(),
        },
    );
    response.is_ok_and(|response| {
        response
            .metadata
            .locale
            .is_none_or(|resolved| resolved.language == requested.language)
    })
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
    let candidates = requested
        .iter()
        .cloned()
        .map(|requested| NumberFormatLocaleCandidate {
            supported: supports_number_format_locale(requested.locale()),
            requested,
        })
        .collect::<Vec<_>>();
    let selected = candidates
        .iter()
        .find(|candidate| candidate.supported)
        .map(|candidate| candidate.requested.clone());
    let used_default = selected.is_none();
    NumberFormatLocaleNegotiation {
        matcher,
        candidates,
        selected: selected
            .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid")),
        used_default,
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
    negotiate_number_format_locale(requested, matcher)
        .candidates
        .into_iter()
        .filter(|candidate| candidate.supported)
        .map(|candidate| candidate.requested)
        .collect()
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

/// The initially data-backed subset of sanctioned single-unit identifiers.
///
/// These are the ten unit values required by `Intl.DurationFormat`'s
/// `PartitionDurationFormatPattern` use of `Intl.NumberFormat`. The enum is
/// deliberately independent of DurationFormat so that new sanctioned units
/// and compound-unit patterns can be added without changing its API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberFormatUnit {
    /// A percentage unit.
    Percent,
    /// Calendar years.
    Year,
    /// Calendar months.
    Month,
    /// Seven-day weeks.
    Week,
    /// Days.
    Day,
    /// Hours.
    Hour,
    /// Minutes.
    Minute,
    /// Seconds.
    Second,
    /// Milliseconds.
    Millisecond,
    /// Microseconds.
    Microsecond,
    /// Nanoseconds.
    Nanosecond,
}

impl NumberFormatUnit {
    /// Parses one data-backed sanctioned single-unit identifier.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "percent" => Some(Self::Percent),
            "year" => Some(Self::Year),
            "month" => Some(Self::Month),
            "week" => Some(Self::Week),
            "day" => Some(Self::Day),
            "hour" => Some(Self::Hour),
            "minute" => Some(Self::Minute),
            "second" => Some(Self::Second),
            "millisecond" => Some(Self::Millisecond),
            "microsecond" => Some(Self::Microsecond),
            "nanosecond" => Some(Self::Nanosecond),
            _ => None,
        }
    }

    /// Returns the sanctioned single-unit identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Percent => "percent",
            Self::Year => "year",
            Self::Month => "month",
            Self::Week => "week",
            Self::Day => "day",
            Self::Hour => "hour",
            Self::Minute => "minute",
            Self::Second => "second",
            Self::Millisecond => "millisecond",
            Self::Microsecond => "microsecond",
            Self::Nanosecond => "nanosecond",
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

/// The ECMA-402 type of one number-format part.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumberFormatPartKind {
    /// The negative sign.
    MinusSign,
    /// The positive sign.
    PlusSign,
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
    negotiation: NumberFormatLocaleNegotiation,
    resolved: ResolvedNumberFormatOptions,
    rounding_increment: u16,
    significant_digits: Option<(u8, u8)>,
    trailing_zero_display: NumberTrailingZeroDisplay,
    compact_display: NumberCompactDisplay,
    currency: Option<NumberCurrencyOptions>,
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
            let digits = currency
                .as_ref()
                .map_or(2, |currency| currency_digits(&currency.code));
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
        let selected = negotiation.selected.clone();
        let provider = NumberingSystemInspectionProvider::default();
        let mut formatter_options = DecimalFormatterOptions::default();
        formatter_options.grouping_strategy = Some(options.use_grouping.into());
        let mut preferences: DecimalFormatterPreferences = selected.locale().into();
        if let Some(numbering_system) = unicode_keyword(selected.locale(), "nu") {
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
        let numbering_system = provider
            .numbering_system
            .into_inner()
            .unwrap_or_else(|| "latn".into());
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
        Ok(Self {
            formatter,
            negotiation,
            resolved,
            rounding_increment,
            significant_digits,
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

    /// Formats an already-coerced finite decimal string into ECMA-402 parts.
    pub fn format_to_parts_decimal(
        &self,
        value: &str,
    ) -> Result<Vec<NumberFormatPart>, NumberFormatError> {
        let value = number_sign_display_decimal(value, self.resolved.sign_display);
        let decimal =
            Decimal::try_from_str(value).map_err(|_| NumberFormatError::InvalidDecimal)?;
        self.format_decimal_value(decimal)
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
    }

    fn format_decimal_value(
        &self,
        mut value: Decimal,
    ) -> Result<Vec<NumberFormatPart>, NumberFormatError> {
        if self.resolved.style == NumberFormatStyle::Percent {
            value.multiply_pow10(2);
        }
        if let Some((minimum, maximum)) = self.significant_digits {
            value.round_with_mode(
                value.nonzero_magnitude_start() - i16::from(maximum) + 1,
                self.resolved.rounding_mode.fixed_decimal_mode(),
            );
            let minimum_position = value.nonzero_magnitude_start() - i16::from(minimum) + 1;
            value.pad_end(minimum_position);
        } else {
            let (increment, position_adjustment) =
                rounding_increment_parts(self.rounding_increment)
                    .expect("validated at construction");
            value.round_with_mode_and_increment(
                -(self.resolved.maximum_fraction_digits as i16) + position_adjustment,
                self.resolved.rounding_mode.fixed_decimal_mode(),
                increment,
            );
            value.pad_end(-(self.resolved.minimum_fraction_digits as i16));
        }
        if self.trailing_zero_display == NumberTrailingZeroDisplay::StripIfInteger {
            value.trim_end_if_integer();
        }
        value.pad_start(self.resolved.minimum_integer_digits.into());
        let singular = decimal_is_one(&value);
        let mut collector = NumberPartCollector::default();
        self.formatter
            .format(&value)
            .write_to_parts(&mut collector)
            .map_err(|_| NumberFormatError::FormattingFailed)?;
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
        if let Some(currency) = self.currency.as_ref() {
            apply_currency_pattern(
                &mut collector.parts,
                currency,
                &self.resolved.locale,
                negative,
            );
        } else if self.resolved.style == NumberFormatStyle::Percent {
            apply_percent_pattern(&mut collector.parts, &self.resolved.locale);
        } else if let Some(unit) = self.resolved.unit {
            let (separator, label) =
                english_unit_pattern(unit, self.resolved.unit_display, singular);
            if !separator.is_empty() {
                collector.push(NumberFormatPartKind::Literal, separator);
            }
            collector.push(NumberFormatPartKind::Unit, label);
        }
        Ok(collector.parts)
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
            apply_currency_pattern(&mut parts, currency, &self.resolved.locale, negative);
        } else if self.resolved.style == NumberFormatStyle::Percent {
            apply_percent_pattern(&mut parts, &self.resolved.locale);
        } else if let Some(unit) = self.resolved.unit {
            let (separator, label) = english_unit_pattern(unit, self.resolved.unit_display, false);
            if !separator.is_empty() {
                parts.push(NumberFormatPart {
                    kind: NumberFormatPartKind::Literal,
                    value: separator.into(),
                });
            }
            parts.push(NumberFormatPart {
                kind: NumberFormatPartKind::Unit,
                value: label.into(),
            });
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

fn decimal_is_one(value: &Decimal) -> bool {
    let text = value.to_string();
    let text = text.trim_start_matches(['-', '+']);
    text == "1"
        || text.strip_prefix("1.").is_some_and(|fraction| {
            !fraction.is_empty() && fraction.bytes().all(|digit| digit == b'0')
        })
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

/// Returns the English CLDR unit suffix used by the current duration-unit
/// locale-data slice. It is shared with DurationFormat to ensure the standard
/// delegation path and direct service output cannot diverge.
pub(crate) fn english_unit_pattern(
    unit: NumberFormatUnit,
    display: NumberUnitDisplay,
    singular: bool,
) -> (&'static str, &'static str) {
    match display {
        NumberUnitDisplay::Long => (
            " ",
            match (unit, singular) {
                (NumberFormatUnit::Percent, _) => "percent",
                (NumberFormatUnit::Year, true) => "year",
                (NumberFormatUnit::Month, true) => "month",
                (NumberFormatUnit::Week, true) => "week",
                (NumberFormatUnit::Day, true) => "day",
                (NumberFormatUnit::Hour, true) => "hour",
                (NumberFormatUnit::Minute, true) => "minute",
                (NumberFormatUnit::Second, true) => "second",
                (NumberFormatUnit::Millisecond, true) => "millisecond",
                (NumberFormatUnit::Microsecond, true) => "microsecond",
                (NumberFormatUnit::Nanosecond, true) => "nanosecond",
                (NumberFormatUnit::Year, false) => "years",
                (NumberFormatUnit::Month, false) => "months",
                (NumberFormatUnit::Week, false) => "weeks",
                (NumberFormatUnit::Day, false) => "days",
                (NumberFormatUnit::Hour, false) => "hours",
                (NumberFormatUnit::Minute, false) => "minutes",
                (NumberFormatUnit::Second, false) => "seconds",
                (NumberFormatUnit::Millisecond, false) => "milliseconds",
                (NumberFormatUnit::Microsecond, false) => "microseconds",
                (NumberFormatUnit::Nanosecond, false) => "nanoseconds",
            },
        ),
        NumberUnitDisplay::Short => (
            if unit == NumberFormatUnit::Percent {
                ""
            } else {
                " "
            },
            match (unit, singular) {
                (NumberFormatUnit::Percent, _) => "%",
                (NumberFormatUnit::Year, true) => "yr",
                (NumberFormatUnit::Year, false) => "yrs",
                (NumberFormatUnit::Month, true) => "mth",
                (NumberFormatUnit::Month, false) => "mths",
                (NumberFormatUnit::Week, true) => "wk",
                (NumberFormatUnit::Week, false) => "wks",
                (NumberFormatUnit::Day, true) => "day",
                (NumberFormatUnit::Day, false) => "days",
                (NumberFormatUnit::Hour, _) => "hr",
                (NumberFormatUnit::Minute, _) => "min",
                (NumberFormatUnit::Second, _) => "sec",
                (NumberFormatUnit::Millisecond, _) => "ms",
                (NumberFormatUnit::Microsecond, _) => "μs",
                (NumberFormatUnit::Nanosecond, _) => "ns",
            },
        ),
        NumberUnitDisplay::Narrow => (
            "",
            match unit {
                NumberFormatUnit::Percent => "%",
                NumberFormatUnit::Year => "y",
                NumberFormatUnit::Month => "m",
                NumberFormatUnit::Week => "w",
                NumberFormatUnit::Day => "d",
                NumberFormatUnit::Hour => "h",
                NumberFormatUnit::Minute => "m",
                NumberFormatUnit::Second => "s",
                NumberFormatUnit::Millisecond => "ms",
                NumberFormatUnit::Microsecond => "μs",
                NumberFormatUnit::Nanosecond => "ns",
            },
        ),
    }
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

fn currency_digits(code: &str) -> u8 {
    match code {
        "BHD" | "JOD" | "KWD" | "OMR" | "TND" => 3,
        "CLF" => 4,
        "JPY" | "KRW" => 0,
        _ => 2,
    }
}

fn apply_currency_pattern(
    parts: &mut Vec<NumberFormatPart>,
    currency: &NumberCurrencyOptions,
    locale: &str,
    negative: bool,
) {
    let accounting =
        negative && currency.sign == NumberCurrencySign::Accounting && !locale.starts_with("de");
    if accounting {
        parts.retain(|part| part.kind != NumberFormatPartKind::MinusSign);
        parts.insert(
            0,
            NumberFormatPart {
                kind: NumberFormatPartKind::Literal,
                value: "(".into(),
            },
        );
    }

    let symbol = currency_symbol(currency, locale);
    let currency_part = NumberFormatPart {
        kind: NumberFormatPartKind::Currency,
        value: symbol,
    };
    if locale.starts_with("de") {
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
    if accounting {
        parts.push(NumberFormatPart {
            kind: NumberFormatPartKind::Literal,
            value: ")".into(),
        });
    }
}

fn apply_percent_pattern(parts: &mut Vec<NumberFormatPart>, locale: &str) {
    if locale.starts_with("de") {
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

fn currency_symbol(currency: &NumberCurrencyOptions, locale: &str) -> String {
    match currency.display {
        NumberCurrencyDisplay::Code => currency.code.clone(),
        NumberCurrencyDisplay::Name => match currency.code.as_str() {
            "USD" => "US dollars".into(),
            "EUR" => "euros".into(),
            _ => currency.code.clone(),
        },
        NumberCurrencyDisplay::Symbol | NumberCurrencyDisplay::NarrowSymbol => {
            match currency.code.as_str() {
                "USD" if locale.starts_with("ko") || locale.starts_with("zh") => "US$".into(),
                "USD" => "$".into(),
                "EUR" => "€".into(),
                "JPY" => "¥".into(),
                "KRW" => "₩".into(),
                _ => currency.code.clone(),
            }
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
