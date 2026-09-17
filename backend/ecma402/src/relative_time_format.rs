// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral `Intl.RelativeTimeFormat` service and locale patterns.

use super::*;

/// A singular ECMA-402 relative-time unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelativeTimeUnit {
    /// Seconds.
    Second,
    /// Minutes.
    Minute,
    /// Hours.
    Hour,
    /// Days.
    Day,
    /// Weeks.
    Week,
    /// Months.
    Month,
    /// Quarters.
    Quarter,
    /// Years.
    Year,
}

impl RelativeTimeUnit {
    /// Parses a singular or plural ECMA-402 relative-time unit.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "second" | "seconds" => Some(Self::Second),
            "minute" | "minutes" => Some(Self::Minute),
            "hour" | "hours" => Some(Self::Hour),
            "day" | "days" => Some(Self::Day),
            "week" | "weeks" => Some(Self::Week),
            "month" | "months" => Some(Self::Month),
            "quarter" | "quarters" => Some(Self::Quarter),
            "year" | "years" => Some(Self::Year),
            _ => None,
        }
    }

    /// Returns the singular ECMA-402 unit spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Second => "second",
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Quarter => "quarter",
            Self::Year => "year",
        }
    }
}

/// The `style` option accepted by `Intl.RelativeTimeFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RelativeTimeStyle {
    /// The ordinary CLDR relative-time pattern.
    #[default]
    Long,
    /// The abbreviated CLDR relative-time pattern.
    Short,
    /// The narrow CLDR relative-time pattern.
    Narrow,
}

/// The `numeric` option accepted by `Intl.RelativeTimeFormat`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RelativeTimeNumeric {
    /// Always render the numeric relative-time pattern.
    #[default]
    Always,
    /// Use a locale's qualitative relative-time terms where available.
    Auto,
}

/// Host-neutral options for `Intl.RelativeTimeFormat`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RelativeTimeFormatOptions {
    /// The requested locale matching policy.
    pub locale_matcher: LocaleMatcher,
    /// A valid numbering-system identifier requested by the embedding host.
    /// Unsupported values resolve to the locale default.
    pub numbering_system: Option<String>,
    /// The relative-time pattern width.
    pub style: RelativeTimeStyle,
    /// Whether qualitative terms may replace a numeric pattern.
    pub numeric: RelativeTimeNumeric,
}

/// ECMAScript-observable data resolved by a relative-time service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedRelativeTimeFormatOptions {
    /// The negotiated locale after any accepted numbering-system override.
    pub locale: String,
    /// The selected numbering system.
    pub numbering_system: String,
    /// The selected relative-time pattern width.
    pub style: RelativeTimeStyle,
    /// The selected qualitative/numeric policy.
    pub numeric: RelativeTimeNumeric,
}

/// A part of a relative-time formatted result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelativeTimePart {
    /// The ECMA-402 `formatToParts` type.
    pub kind: RelativeTimePartKind,
    /// The text of this part.
    pub value: String,
}

/// The kind of a [`RelativeTimePart`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelativeTimePartKind {
    /// Locale pattern text outside the number.
    Literal,
    /// A contiguous integer digit run.
    Integer,
    /// A grouping separator.
    Group,
    /// A decimal separator.
    Decimal,
    /// A contiguous fraction digit run.
    Fraction,
}

/// A relative-time construction or formatting failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelativeTimeFormatError {
    /// The selected locale's numeric data was unavailable.
    DataUnavailable,
    /// The numeric input was `NaN` or infinite.
    NonFiniteNumber,
}

impl std::fmt::Display for RelativeTimeFormatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataUnavailable => formatter.write_str("relative-time data is unavailable"),
            Self::NonFiniteNumber => formatter.write_str("relative time must be finite"),
        }
    }
}

impl std::error::Error for RelativeTimeFormatError {}

/// Returns whether the bundled relative-time patterns support this locale.
///
/// The pinned CLDR provider supplies patterns for every resolved locale in its
/// 766-locale inventory. A locale is advertised only after its mandatory
/// numeric `other` pattern has been found in that provider.
pub fn supports_relative_time_format_locale(locale: &IcuLocale) -> bool {
    locale_data_provider().supports_service_locale(IntlService::RelativeTimeFormat, locale)
}

/// Returns requested locales supported by the bundled relative-time service.
pub fn supported_relative_time_format_locales(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    supported_locales(IntlService::RelativeTimeFormat, requested, matcher)
}

fn resolve_relative_time_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    resolve_locale(IntlService::RelativeTimeFormat, requested, matcher)
        .selected()
        .clone()
}

/// A host-neutral `Intl.RelativeTimeFormat` service.
///
/// It owns locale-data lookup and partitioning. The embedding runtime remains
/// responsible for ECMAScript `ToNumber`/`ToString` coercion and result-object
/// construction.
pub struct RelativeTimeFormat {
    number_format: NumberFormat,
    resolved: ResolvedRelativeTimeFormatOptions,
}

impl RelativeTimeFormat {
    /// Constructs a relative-time formatter from typed options.
    pub fn try_new(
        requested: &[CanonicalLocale],
        options: RelativeTimeFormatOptions,
    ) -> Result<Self, RelativeTimeFormatError> {
        let matched = resolve_relative_time_format_locale(requested, options.locale_matcher);
        let locale = resolve_numbering_system_locale(&matched, options.numbering_system.as_deref());
        let number_format = NumberFormat::try_new(
            std::slice::from_ref(&locale),
            NumberFormatOptions {
                locale_matcher: options.locale_matcher,
                ..Default::default()
            },
        )
        .map_err(|_| RelativeTimeFormatError::DataUnavailable)?;
        let resolved = ResolvedRelativeTimeFormatOptions {
            locale: locale.as_str().to_owned(),
            numbering_system: number_format.resolved_options().numbering_system.clone(),
            style: options.style,
            numeric: options.numeric,
        };
        Ok(Self {
            number_format,
            resolved,
        })
    }

    /// Formats a finite relative-time quantity into ECMA-402-style parts.
    pub fn format_to_parts(
        &self,
        value: f64,
        unit: RelativeTimeUnit,
    ) -> Result<Vec<RelativeTimePart>, RelativeTimeFormatError> {
        if !value.is_finite() {
            return Err(RelativeTimeFormatError::NonFiniteNumber);
        }
        let provider = crate::locale_data_provider();
        if let Some(term) = provider.relative_time_qualitative_term(
            &self.resolved.locale,
            self.resolved.style,
            self.resolved.numeric,
            value,
            unit,
        ) {
            return Ok(vec![RelativeTimePart {
                kind: RelativeTimePartKind::Literal,
                value: term,
            }]);
        }
        // ECMA-402 uses the mathematical sign of the original Number when
        // selecting `past` or `future`; -0 is consequently a past value even
        // though its formatted magnitude is zero.
        let past = value.is_sign_negative();
        let plural = provider.relative_time_plural_category(&self.resolved.locale, value.abs());
        let pattern = provider
            .relative_time_pattern(
                &self.resolved.locale,
                self.resolved.style,
                unit,
                past,
                plural,
            )
            .or_else(|| {
                provider.relative_time_pattern(
                    &self.resolved.locale,
                    self.resolved.style,
                    unit,
                    past,
                    crate::PluralCategory::Other,
                )
            })
            .ok_or(RelativeTimeFormatError::DataUnavailable)?;
        let number = self
            .number_format
            .format_to_parts_f64(value.abs())
            .map_err(|_| RelativeTimeFormatError::NonFiniteNumber)?;
        relative_time_pattern_parts(&pattern, number)
    }

    /// Formats a finite relative-time quantity into a string.
    pub fn format(
        &self,
        value: f64,
        unit: RelativeTimeUnit,
    ) -> Result<String, RelativeTimeFormatError> {
        Ok(self
            .format_to_parts(value, unit)?
            .into_iter()
            .map(|part| part.value)
            .collect())
    }

    /// Returns the resolved service options.
    pub fn resolved_options(&self) -> &ResolvedRelativeTimeFormatOptions {
        &self.resolved
    }

    /// Returns heap storage directly owned by this service.
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.number_format.bytes()
            + self.resolved.locale.len()
            + self.resolved.numbering_system.len()
    }
}

fn relative_time_pattern_parts(
    pattern: &str,
    number: Vec<crate::NumberFormatPart>,
) -> Result<Vec<RelativeTimePart>, RelativeTimeFormatError> {
    // Some CLDR plural forms (for example Arabic `one` and `two`) express the
    // quantity in words and intentionally omit `{0}`. ECMA-402 exposes those
    // complete patterns as one literal part rather than synthesizing digits.
    let Some((prefix, suffix)) = pattern.split_once("{0}") else {
        return Ok(vec![RelativeTimePart {
            kind: RelativeTimePartKind::Literal,
            value: pattern.into(),
        }]);
    };
    let mut parts = Vec::new();
    if !prefix.is_empty() {
        parts.push(RelativeTimePart {
            kind: RelativeTimePartKind::Literal,
            value: prefix.into(),
        });
    }
    parts.extend(number.into_iter().map(|part| RelativeTimePart {
        kind: match part.kind {
            crate::NumberFormatPartKind::Integer => RelativeTimePartKind::Integer,
            crate::NumberFormatPartKind::Group => RelativeTimePartKind::Group,
            crate::NumberFormatPartKind::Decimal => RelativeTimePartKind::Decimal,
            crate::NumberFormatPartKind::Fraction => RelativeTimePartKind::Fraction,
            crate::NumberFormatPartKind::MinusSign
            | crate::NumberFormatPartKind::PlusSign
            | crate::NumberFormatPartKind::ApproximatelySign
            | crate::NumberFormatPartKind::ExponentSeparator
            | crate::NumberFormatPartKind::ExponentMinusSign
            | crate::NumberFormatPartKind::ExponentInteger
            | crate::NumberFormatPartKind::Literal
            | crate::NumberFormatPartKind::Unit
            | crate::NumberFormatPartKind::Currency
            | crate::NumberFormatPartKind::PercentSign
            | crate::NumberFormatPartKind::Compact
            | crate::NumberFormatPartKind::Nan
            | crate::NumberFormatPartKind::Infinity => RelativeTimePartKind::Literal,
        },
        value: part.value,
    }));
    if !suffix.is_empty() {
        parts.push(RelativeTimePart {
            kind: RelativeTimePartKind::Literal,
            value: suffix.into(),
        });
    }
    Ok(parts)
}
