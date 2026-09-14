// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral registry for `Intl.supportedValuesOf` data that is available
//! to the currently implemented ECMA-402 services.

/// Calendar identifiers backed by `Intl.DateTimeFormat`.
pub const SUPPORTED_CALENDARS: &[&str] = &[
    "buddhist",
    "chinese",
    "coptic",
    "dangi",
    "ethioaa",
    "ethiopic",
    "gregory",
    "hebrew",
    "indian",
    "islamic-civil",
    "islamic-tbla",
    "islamic-umalqura",
    "iso8601",
    "japanese",
    "persian",
    "roc",
];

/// Decimal numbering systems backed by date, number, and relative-time
/// formatting. The list includes every ECMA-402 simple digit mapping.
pub const SUPPORTED_NUMBERING_SYSTEMS: &[&str] = &[
    "adlm", "ahom", "arab", "arabext", "bali", "beng", "bhks", "brah", "cakm", "cham", "deva",
    "diak", "fullwide", "gara", "gong", "gonm", "gujr", "gukh", "guru", "hanidec", "hmng", "hmnp",
    "java", "kali", "kawi", "khmr", "knda", "krai", "lana", "lanatham", "laoo", "latn", "lepc",
    "limb", "mathbold", "mathdbl", "mathmono", "mathsanb", "mathsans", "mlym", "modi", "mong",
    "mroo", "mtei", "mymr", "mymrepka", "mymrpao", "mymrshan", "mymrtlng", "nagm", "newa", "nkoo",
    "olck", "onao", "orya", "osma", "outlined", "rohg", "saur", "segment", "shrd", "sind", "sinh",
    "sora", "sund", "sunu", "takr", "talu", "tamldec", "telu", "thai", "tibt", "tirh", "tnsa",
    "tols", "vaii", "wara", "wcho",
];

/// Collation types that can resolve through `Intl.Collator` for at least one
/// bundled locale.
pub const SUPPORTED_COLLATIONS: &[&str] = &[
    "compat", "dict", "emoji", "eor", "phonebk", "pinyin", "searchjl", "stroke", "trad", "unihan",
    "zhuyin",
];

/// ISO 4217 codes with localized `Intl.DisplayNames` data.
pub const SUPPORTED_CURRENCIES: &[&str] = &["EUR", "JPY", "USD"];

/// Sanctioned simple units whose NumberFormat patterns are implemented.
pub const SUPPORTED_UNITS: &[&str] = &[
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

/// Whether a numbering-system identifier has data in every service that
/// `Intl.supportedValuesOf("numberingSystem")` promises.
pub fn supports_numbering_system(value: &str) -> bool {
    SUPPORTED_NUMBERING_SYSTEMS.contains(&value)
}

/// A request key not carried by the current ECMA-402 data registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportedValuesError {
    /// The key is not one of ECMA-402's supported-values categories.
    InvalidKey,
    /// The category is specified, but its complete data registry is not yet
    /// bundled by this host-neutral implementation.
    DataUnavailable,
}

impl std::fmt::Display for SupportedValuesError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKey => formatter.write_str("invalid Intl.supportedValuesOf key"),
            Self::DataUnavailable => {
                formatter.write_str("Intl.supportedValuesOf data is unavailable")
            }
        }
    }
}

impl std::error::Error for SupportedValuesError {}

/// Returns the canonical values in a registry category.
///
/// The pinned `jiff-tzdb` data is also the database DateTimeFormat uses for
/// zone lookup, so its complete Zone-and-Link inventory can safely be exposed
/// for `timeZone`. The UTC-equivalent legacy spellings that ECMA-402
/// canonicalizes are folded to `UTC`; every other accepted IANA spelling is retained. This is
/// important because modern `CreateDateTimeFormat` retains a Link name in its
/// internal `[[TimeZone]]` slot instead of resolving it to its target.
///
/// Every returned category is derived from the formatter or display-name data
/// that consumes it, so callers never receive an identifier which the
/// corresponding ECMA-402 service cannot resolve.
pub fn supported_values_of(key: &str) -> Result<Vec<String>, SupportedValuesError> {
    match key {
        "calendar" => Ok(SUPPORTED_CALENDARS
            .iter()
            .map(ToString::to_string)
            .collect()),
        "collation" => Ok(SUPPORTED_COLLATIONS
            .iter()
            .map(ToString::to_string)
            .collect()),
        "currency" => Ok(SUPPORTED_CURRENCIES
            .iter()
            .map(ToString::to_string)
            .collect()),
        "numberingSystem" => Ok(SUPPORTED_NUMBERING_SYSTEMS
            .iter()
            .map(ToString::to_string)
            .collect()),
        "timeZone" => {
            let mut values = jiff_tzdb::available()
                .map(canonical_supported_time_zone)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            values.sort_unstable();
            values.dedup();
            Ok(values)
        }
        "unit" => Ok(SUPPORTED_UNITS.iter().map(ToString::to_string).collect()),
        _ => Err(SupportedValuesError::InvalidKey),
    }
}

fn canonical_supported_time_zone(identifier: &str) -> &str {
    match identifier {
        "Etc/GMT" | "Etc/GMT0" | "Etc/UTC" | "GMT" | "GMT0" => "UTC",
        _ => identifier,
    }
}
