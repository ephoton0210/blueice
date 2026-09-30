// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Primary time zone identifiers: ECMA-402's `AvailableNamedTimeZoneIdentifiers`.
//!
//! Every IANA Zone or Link name has a *primary identifier*. Two names denote
//! the same zone (`TimeZoneEquals`) exactly when their primary identifiers
//! are equal, and `Intl.supportedValuesOf("timeZone")` lists the identifiers
//! that are their own primary.
//!
//! The primary identifier is **not** recoverable from the TZif data the
//! `jiff-tzdb` bundle carries: the tz database stores a Link's data as its
//! target's, so `Africa/Accra` and `Africa/Abidjan` are byte-identical there,
//! yet `Africa/Accra` is listed in `zone.tab` and is therefore its own primary
//! identifier, distinct from `Africa/Abidjan`. The bundle also records no
//! Link/Zone distinction at all. The relationships live in a generated table
//! (`table.rs`) covering the names that are *not* their own primary; every other
//! bundled name is primary.
//!
//! The table is derived per ECMA-402's algorithm by
//! `backend/ecma402/tools/generate_time_zone_identifiers.mjs` (see its header
//! for the inputs and how to rebuild), and reproduces every row of Test262's
//! `intl402/Temporal/ZonedDateTime/links.js`.
//!
//! Provenance of the checked-in table (tz database release 2026c): the Zone and
//! Link relationships come from the IANA default build's `tzdata.zi` (as
//! shipped by the PyPI `tzdata` 2026.3 package), the per-country Link targets
//! from Debian's `tzdata` 2026c `tzdata.zi` (built with
//! `PACKRATDATA=backzone PACKRATLIST=zone.tab`), and the country of each zone
//! from that release's `zone.tab`. To regenerate after a `jiff-tzdb` bump,
//! obtain those three files for the new release and run the script (its
//! `--links-js` check compares the result against Test262's own table). The
//! table is tied to one tz database
//! release (`TIME_ZONE_TABLE_TZDATA_RELEASE`); a unit test fails when the bundled
//! `jiff-tzdb` moves to another release, which is the cue to regenerate it.

mod table;

use table::NON_PRIMARY_IDENTIFIERS;

/// The tz database release the primary-identifier table was generated from,
/// which must be the release `jiff-tzdb` bundles.
pub const TIME_ZONE_TABLE_TZDATA_RELEASE: &str = table::TZDATA_VERSION;

/// The bundled database's spelling of `identifier` (matched ASCII
/// case-insensitively, as `GetAvailableNamedTimeZoneIdentifier` does), or
/// `None` for a name the database does not contain.
fn available_name(identifier: &str) -> Option<&'static str> {
    jiff_tzdb::get(identifier).map(|(name, _)| name)
}

/// The primary identifier of an IANA Zone or Link name: `Asia/Kolkata` for
/// `Asia/Calcutta`, `UTC` for `Etc/GMT`, and the name itself (in the
/// database's own spelling) for a name that is primary.
///
/// `None` when `identifier` is not a name in the bundled database.
pub fn primary_time_zone_identifier(identifier: &str) -> Option<&'static str> {
    let name = available_name(identifier)?;
    Some(
        NON_PRIMARY_IDENTIFIERS
            .binary_search_by_key(&name, |&(alias, _)| alias)
            .map_or(name, |index| NON_PRIMARY_IDENTIFIERS[index].1),
    )
}

/// Whether `identifier` names a bundled zone that is its own primary
/// identifier -- exactly what `Intl.supportedValuesOf("timeZone")` lists.
pub fn is_primary_time_zone_identifier(identifier: &str) -> bool {
    available_name(identifier).is_some_and(|name| {
        NON_PRIMARY_IDENTIFIERS
            .binary_search_by_key(&name, |&(alias, _)| alias)
            .is_err()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_matches_the_bundled_tz_database_release() {
        // Regenerate the table (see the generator's header) when this fails.
        assert_eq!(jiff_tzdb::VERSION, Some(TIME_ZONE_TABLE_TZDATA_RELEASE));
    }

    #[test]
    fn the_table_is_sorted_by_name_without_duplicates() {
        assert!(NON_PRIMARY_IDENTIFIERS
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn every_name_in_the_table_is_a_bundled_name_spelled_as_the_bundle_spells_it() {
        for (alias, primary) in NON_PRIMARY_IDENTIFIERS {
            assert_eq!(available_name(alias), Some(alias), "{alias}");
            assert_eq!(available_name(primary), Some(primary), "{primary}");
        }
    }

    #[test]
    fn a_primary_identifier_is_never_itself_an_alias() {
        for (alias, primary) in NON_PRIMARY_IDENTIFIERS {
            assert_ne!(alias, primary);
            assert!(
                is_primary_time_zone_identifier(primary),
                "{alias} -> {primary}"
            );
            assert_eq!(primary_time_zone_identifier(primary), Some(primary));
        }
    }

    #[test]
    fn counts_agree_with_the_bundle() {
        let names = jiff_tzdb::available().count();
        let primaries = jiff_tzdb::available()
            .filter(|name| is_primary_time_zone_identifier(name))
            .count();
        assert_eq!(names, 598);
        assert_eq!(names - primaries, NON_PRIMARY_IDENTIFIERS.len());
        assert_eq!(primaries, 446);
    }

    #[test]
    fn utc_is_primary_and_the_rest_of_its_group_is_not() {
        assert_eq!(primary_time_zone_identifier("UTC"), Some("UTC"));
        assert_eq!(primary_time_zone_identifier("etc/gmt"), Some("UTC"));
        assert!(is_primary_time_zone_identifier("utc"));
        assert!(!is_primary_time_zone_identifier("ETC/UTC"));
    }
}
