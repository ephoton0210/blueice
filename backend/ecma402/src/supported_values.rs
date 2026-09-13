// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral registry for `Intl.supportedValuesOf` data that is available
//! to the currently implemented ECMA-402 services.

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
/// The initial registry deliberately exposes only `latn`: it is the one
/// numbering system implemented by the DurationFormat formatter, so every
/// advertised value has matching constructor and formatting support. Other
/// standardized categories remain unavailable until their complete data sets
/// can be supplied; returning a partial global list for those categories would
/// violate `Intl.supportedValuesOf`'s cross-service contract.
pub fn supported_values_of(key: &str) -> Result<&'static [&'static str], SupportedValuesError> {
    match key {
        "numberingSystem" => Ok(&["latn"]),
        "calendar" | "collation" | "currency" | "timeZone" | "unit" => {
            Err(SupportedValuesError::DataUnavailable)
        }
        _ => Err(SupportedValuesError::InvalidKey),
    }
}
