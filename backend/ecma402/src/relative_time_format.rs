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
/// The service intentionally advertises only languages with bundled patterns;
/// NumberFormat's broader data coverage must not be mistaken for relative-time
/// data coverage.
pub fn supports_relative_time_format_locale(locale: &IcuLocale) -> bool {
    supports_locale_language(locale)
}

/// Returns requested locales supported by the bundled relative-time service.
pub fn supported_relative_time_format_locales(
    requested: &[CanonicalLocale],
    _matcher: LocaleMatcher,
) -> Vec<CanonicalLocale> {
    requested
        .iter()
        .filter(|locale| supports_relative_time_format_locale(locale.locale()))
        .cloned()
        .collect()
}

fn resolve_relative_time_format_locale(
    requested: &[CanonicalLocale],
    matcher: LocaleMatcher,
) -> CanonicalLocale {
    supported_relative_time_format_locales(requested, matcher)
        .into_iter()
        .next()
        .unwrap_or_else(|| canonicalize("en-US").expect("the default locale is valid"))
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
        let mut locale = resolve_relative_time_format_locale(requested, options.locale_matcher);
        let requested_numbering = options
            .numbering_system
            .as_deref()
            .filter(|value| supports_numbering_system(value));
        let extension_numbering =
            unicode_keyword(locale.locale(), "nu").filter(|value| supports_numbering_system(value));
        let numbering_system = requested_numbering
            .or(extension_numbering.as_deref())
            .unwrap_or("latn");
        let retain_extension = extension_numbering.as_deref() == Some(numbering_system);
        locale = locale_with_numbering_system(&locale, numbering_system, retain_extension);
        let number_format = NumberFormat::try_new(
            &[locale.clone()],
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
        if let Some(term) = self.qualitative_term(value, unit) {
            return Ok(vec![RelativeTimePart {
                kind: RelativeTimePartKind::Literal,
                value: term.into(),
            }]);
        }
        let past = value.is_sign_negative();
        let number = self
            .number_format
            .format_f64(value.abs())
            .map_err(|_| RelativeTimeFormatError::NonFiniteNumber)?;
        let mut parts = Vec::new();
        if !past {
            parts.push(RelativeTimePart {
                kind: RelativeTimePartKind::Literal,
                value: if self.resolved.locale.starts_with("pl") {
                    "za ".into()
                } else {
                    "in ".into()
                },
            });
        }
        parts.extend(relative_time_number_parts(
            &number,
            self.resolved.locale.starts_with("pl"),
        ));
        let label = self.unit_label(value.abs(), unit);
        parts.push(RelativeTimePart {
            kind: RelativeTimePartKind::Literal,
            value: if past {
                if self.resolved.locale.starts_with("pl") {
                    format!(" {label} temu")
                } else {
                    format!(" {label} ago")
                }
            } else {
                format!(" {label}")
            },
        });
        Ok(parts)
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

    fn qualitative_term(&self, value: f64, unit: RelativeTimeUnit) -> Option<&'static str> {
        if self.resolved.numeric != RelativeTimeNumeric::Auto
            || !self.resolved.locale.starts_with("en")
        {
            return None;
        }
        match (value.is_sign_negative(), value.abs() as i64, unit) {
            (_, 0, RelativeTimeUnit::Second) if value == 0.0 => Some("now"),
            (_, 0, RelativeTimeUnit::Minute) if value == 0.0 => Some("this minute"),
            (_, 0, RelativeTimeUnit::Hour) if value == 0.0 => Some("this hour"),
            (_, 0, RelativeTimeUnit::Day) if value == 0.0 => Some("today"),
            (false, 1, RelativeTimeUnit::Day) => Some("tomorrow"),
            (true, 1, RelativeTimeUnit::Day) => Some("yesterday"),
            (_, 0, RelativeTimeUnit::Week) if value == 0.0 => Some("this week"),
            (false, 1, RelativeTimeUnit::Week) => Some("next week"),
            (true, 1, RelativeTimeUnit::Week) => Some("last week"),
            (_, 0, RelativeTimeUnit::Month) if value == 0.0 => Some("this month"),
            (false, 1, RelativeTimeUnit::Month) => Some("next month"),
            (true, 1, RelativeTimeUnit::Month) => Some("last month"),
            (_, 0, RelativeTimeUnit::Quarter) if value == 0.0 => Some("this quarter"),
            (false, 1, RelativeTimeUnit::Quarter) => Some("next quarter"),
            (true, 1, RelativeTimeUnit::Quarter) => Some("last quarter"),
            (_, 0, RelativeTimeUnit::Year) if value == 0.0 => Some("this year"),
            (false, 1, RelativeTimeUnit::Year) => Some("next year"),
            (true, 1, RelativeTimeUnit::Year) => Some("last year"),
            _ => None,
        }
    }

    fn unit_label(&self, value: f64, unit: RelativeTimeUnit) -> &'static str {
        if self.resolved.locale.starts_with("pl") {
            return polish_relative_time_label(self.resolved.style, unit, value);
        }
        english_relative_time_label(self.resolved.style, unit, value)
    }
}

fn relative_time_number_parts(number: &str, polish: bool) -> Vec<RelativeTimePart> {
    let mut parts = Vec::new();
    let mut kind = RelativeTimePartKind::Integer;
    let mut buffer = String::new();
    let flush = |parts: &mut Vec<RelativeTimePart>, buffer: &mut String, kind| {
        if !buffer.is_empty() {
            parts.push(RelativeTimePart {
                kind,
                value: std::mem::take(buffer),
            });
        }
    };
    for character in number.chars() {
        let separator = match character {
            ',' if polish => Some(RelativeTimePartKind::Decimal),
            ',' | '\u{a0}' | '\u{202f}' | '\u{66c}' => Some(RelativeTimePartKind::Group),
            '.' | '\u{66b}' => Some(RelativeTimePartKind::Decimal),
            _ => None,
        };
        if let Some(separator) = separator {
            flush(&mut parts, &mut buffer, kind);
            parts.push(RelativeTimePart {
                kind: separator,
                value: character.into(),
            });
            kind = if separator == RelativeTimePartKind::Decimal {
                RelativeTimePartKind::Fraction
            } else {
                RelativeTimePartKind::Integer
            };
        } else {
            buffer.push(character);
        }
    }
    flush(&mut parts, &mut buffer, kind);
    parts
}

fn english_relative_time_label(
    style: RelativeTimeStyle,
    unit: RelativeTimeUnit,
    value: f64,
) -> &'static str {
    let one = value == 1.0;
    match (style, unit, one) {
        (RelativeTimeStyle::Long, RelativeTimeUnit::Second, true) => "second",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Minute, true) => "minute",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Hour, true) => "hour",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Day, true) => "day",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, true) => "week",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, true) => "month",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, true) => "quarter",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, true) => "year",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Second, false) => "seconds",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Minute, false) => "minutes",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Hour, false) => "hours",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Day, false) => "days",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Week, false) => "weeks",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Month, false) => "months",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Quarter, false) => "quarters",
        (RelativeTimeStyle::Long, RelativeTimeUnit::Year, false) => "years",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Second, _) => "sec.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Minute, _) => "min.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Hour, _) => "hr.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Day, true) => "day",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Day, false) => "days",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Week, _) => "wk.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Month, _) => "mo.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Quarter, true) => "qtr.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Quarter, false) => "qtrs.",
        (RelativeTimeStyle::Short, RelativeTimeUnit::Year, _) => "yr.",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Second, _) => "s",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Minute, _) => "m",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Hour, _) => "h",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Day, _) => "d",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Week, _) => "w",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Month, _) => "mo",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Quarter, _) => "q",
        (RelativeTimeStyle::Narrow, RelativeTimeUnit::Year, _) => "y",
    }
}

fn polish_relative_time_label(
    style: RelativeTimeStyle,
    unit: RelativeTimeUnit,
    value: f64,
) -> &'static str {
    #[derive(Clone, Copy)]
    enum Category {
        One,
        Few,
        Many,
        Other,
    }

    let integer = value.fract() == 0.0;
    let number = value as i64;
    let category = if integer && number == 1 {
        Category::One
    } else if integer
        && (2..=4).contains(&(number.rem_euclid(10)))
        && !(12..=14).contains(&(number.rem_euclid(100)))
    {
        Category::Few
    } else if integer {
        Category::Many
    } else {
        Category::Other
    };
    let select = |one, few, many, other| match category {
        Category::One => one,
        Category::Few => few,
        Category::Many => many,
        Category::Other => other,
    };
    match style {
        RelativeTimeStyle::Long => match unit {
            RelativeTimeUnit::Second => select("sekundę", "sekundy", "sekund", "sekundy"),
            RelativeTimeUnit::Minute => select("minutę", "minuty", "minut", "minuty"),
            RelativeTimeUnit::Hour => select("godzinę", "godziny", "godzin", "godziny"),
            RelativeTimeUnit::Day => select("dzień", "dni", "dni", "dnia"),
            RelativeTimeUnit::Week => select("tydzień", "tygodnie", "tygodni", "tygodnia"),
            RelativeTimeUnit::Month => select("miesiąc", "miesiące", "miesięcy", "miesiąca"),
            RelativeTimeUnit::Quarter => select("kwartał", "kwartały", "kwartałów", "kwartału"),
            RelativeTimeUnit::Year => select("rok", "lata", "lat", "roku"),
        },
        RelativeTimeStyle::Short => match unit {
            RelativeTimeUnit::Second => "sek.",
            RelativeTimeUnit::Minute => "min",
            RelativeTimeUnit::Hour => "godz.",
            RelativeTimeUnit::Day => select("dzień", "dni", "dni", "dnia"),
            RelativeTimeUnit::Week => select("tydz.", "tyg.", "tyg.", "tyg."),
            RelativeTimeUnit::Month => "mies.",
            RelativeTimeUnit::Quarter => "kw.",
            RelativeTimeUnit::Year => select("rok", "lata", "lat", "roku"),
        },
        RelativeTimeStyle::Narrow => match unit {
            RelativeTimeUnit::Second => "s",
            RelativeTimeUnit::Minute => "min",
            RelativeTimeUnit::Hour => "g.",
            RelativeTimeUnit::Day => select("dzień", "dni", "dni", "dnia"),
            RelativeTimeUnit::Week => select("tydz.", "tyg.", "tyg.", "tyg."),
            RelativeTimeUnit::Month => "mies.",
            RelativeTimeUnit::Quarter => "kw.",
            RelativeTimeUnit::Year => select("rok", "lata", "lat", "roku"),
        },
    }
}
