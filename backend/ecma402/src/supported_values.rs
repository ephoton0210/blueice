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
/// The pinned `jiff-tzdb` data is also the database DateTimeFormat uses for
/// zone lookup, so its complete Zone-and-Link inventory can safely be exposed
/// for `timeZone`. The UTC-equivalent legacy spellings that ECMA-402
/// canonicalizes are folded to `UTC`; every other accepted IANA spelling is retained. This is
/// important because modern `CreateDateTimeFormat` retains a Link name in its
/// internal `[[TimeZone]]` slot instead of resolving it to its target.
///
/// The remaining categories deliberately advertise only data that is backed
/// by a fully implemented formatter.
pub fn supported_values_of(key: &str) -> Result<Vec<String>, SupportedValuesError> {
    match key {
        "numberingSystem" => Ok(vec!["latn".into()]),
        "timeZone" => {
            let mut values = jiff_tzdb::available()
                .map(canonical_supported_time_zone)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            values.sort_unstable();
            values.dedup();
            Ok(values)
        }
        "calendar" | "collation" | "currency" | "unit" => {
            Err(SupportedValuesError::DataUnavailable)
        }
        _ => Err(SupportedValuesError::InvalidKey),
    }
}

fn canonical_supported_time_zone(identifier: &str) -> &str {
    match identifier {
        "Etc/GMT" | "Etc/GMT0" | "Etc/UTC" | "GMT" | "GMT0" => "UTC",
        _ => identifier,
    }
}
