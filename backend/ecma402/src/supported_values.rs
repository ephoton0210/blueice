// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Intl.supportedValuesOf` view of the shared locale-data provider.

use crate::{locale_data_provider, LocaleDataCategory};

/// Whether a numbering-system identifier has data in every service that
/// `Intl.supportedValuesOf("numberingSystem")` promises.
pub fn supports_numbering_system(value: &str) -> bool {
    locale_data_provider().supports_numbering_system(value)
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
/// The shared provider's pinned `jiff-tzdb` data is also the database
/// DateTimeFormat uses for zone lookup, so its complete Zone-and-Link
/// inventory can safely be exposed for `timeZone`. This is important because
/// modern `CreateDateTimeFormat` retains a Link name in its internal
/// `[[TimeZone]]` slot instead of resolving it to its target.
///
/// Every returned category is derived from the formatter or display-name data
/// that consumes it, so callers never receive an identifier which the
/// corresponding ECMA-402 service cannot resolve.
pub fn supported_values_of(key: &str) -> Result<Vec<String>, SupportedValuesError> {
    match key {
        "calendar" => Ok(locale_data_provider()
            .values(LocaleDataCategory::Calendar)
            .iter()
            .map(ToString::to_string)
            .collect()),
        "collation" => Ok(locale_data_provider()
            .values(LocaleDataCategory::Collation)
            .iter()
            .map(ToString::to_string)
            .collect()),
        "currency" => Ok(locale_data_provider()
            .values(LocaleDataCategory::Currency)
            .iter()
            .map(ToString::to_string)
            .collect()),
        "numberingSystem" => Ok(locale_data_provider()
            .values(LocaleDataCategory::NumberingSystem)
            .iter()
            .map(ToString::to_string)
            .collect()),
        "timeZone" => Ok(locale_data_provider().time_zones()),
        "unit" => Ok(locale_data_provider()
            .values(LocaleDataCategory::Unit)
            .iter()
            .map(ToString::to_string)
            .collect()),
        _ => Err(SupportedValuesError::InvalidKey),
    }
}
