// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ECMA-402's primary time zone identifiers.
//!
//! `AvailableNamedTimeZoneIdentifiers` gives every IANA Zone or Link name a
//! primary identifier, and `Intl.supportedValuesOf("timeZone")` lists exactly
//! the identifiers that are their own primary. Test262 pins both through
//! `intl402/Temporal/ZonedDateTime/{links,prototype/equals/canonical-not-equal,
//! prototype/equals/canonicalize-timezone}.js` and
//! `intl402/Intl/supportedValuesOf/timeZones*.js`; each case below names the
//! fixture whose expectation it restates.

use blueice_ecma402::{
    is_primary_time_zone_identifier, primary_time_zone_identifier, supported_values_of,
};

fn primary(identifier: &str) -> &'static str {
    primary_time_zone_identifier(identifier)
        .unwrap_or_else(|| panic!("{identifier} should be an available identifier"))
}

/// The `backward`-file links are aliases of the zone they name
/// (`links.js`, `canonicalize-iana-identifiers-before-comparing.js`).
#[test]
fn backward_links_share_their_zones_primary_identifier() {
    for (alias, zone) in [
        ("Asia/Calcutta", "Asia/Kolkata"),
        ("US/Pacific", "America/Los_Angeles"),
        ("Australia/Canberra", "Australia/Sydney"),
        ("Europe/Kiev", "Europe/Kyiv"),
        ("Brazil/East", "America/Sao_Paulo"),
        ("Japan", "Asia/Tokyo"),
        ("Asia/Istanbul", "Europe/Istanbul"),
    ] {
        assert_eq!(primary(alias), zone, "{alias}");
        assert_eq!(primary(zone), zone, "{zone} is its own primary");
        assert!(!is_primary_time_zone_identifier(alias), "{alias}");
        assert!(is_primary_time_zone_identifier(zone), "{zone}");
    }
}

/// A name in `zone.tab` is primary even where the tz database merged its data
/// into another zone (`canonical-not-equal.js`, `canonicalize-timezone.js`).
#[test]
fn zone_tab_names_stay_primary_when_tzdata_merged_them_into_a_link() {
    for name in [
        "Africa/Abidjan",
        "Africa/Accra",
        "Africa/Bamako",
        "Europe/Prague",
        "Europe/Bratislava",
        "Europe/Amsterdam",
        "Europe/Brussels",
        "Arctic/Longyearbyen",
        "America/Puerto_Rico",
        "America/St_Thomas",
    ] {
        assert_eq!(primary(name), name);
        assert!(is_primary_time_zone_identifier(name), "{name}");
    }
}

/// A link that crosses a border resolves to the zone of its own country: the
/// only `zone.tab` line for a single-zone country, otherwise the `backzone`
/// target (`canonicalize-timezone.js`, `links.js`).
#[test]
fn a_link_that_crosses_a_border_resolves_to_its_own_countrys_zone() {
    for (alias, zone) in [
        // SJ has one zone.tab line, though `backward` links Jan_Mayen to Europe/Berlin.
        ("Atlantic/Jan_Mayen", "Arctic/Longyearbyen"),
        // ML and IS: one line each, though tzdata links both to Africa/Abidjan.
        ("Africa/Timbuktu", "Africa/Bamako"),
        ("Iceland", "Atlantic/Reykjavik"),
        ("Africa/Asmera", "Africa/Asmara"),
        ("America/Virgin", "America/St_Thomas"),
        // FM has several lines, so the backzone Link lines decide.
        ("Pacific/Truk", "Pacific/Chuuk"),
        ("Pacific/Yap", "Pacific/Chuuk"),
        ("Pacific/Ponape", "Pacific/Pohnpei"),
    ] {
        assert_eq!(primary(alias), zone, "{alias}");
    }
}

/// The names tzdata 2024b turned into links (`links.js`).
#[test]
fn legacy_region_names_are_aliases_of_the_zones_they_link_to() {
    for (alias, zone) in [
        ("CET", "Europe/Brussels"),
        ("MET", "Europe/Brussels"),
        ("EET", "Europe/Athens"),
        ("WET", "Europe/Lisbon"),
        ("EST", "America/Panama"),
        ("MST", "America/Phoenix"),
        ("HST", "Pacific/Honolulu"),
        ("EST5EDT", "America/New_York"),
        ("CST6CDT", "America/Chicago"),
        ("MST7MDT", "America/Denver"),
        ("PST8PDT", "America/Los_Angeles"),
    ] {
        assert_eq!(primary(alias), zone, "{alias}");
    }
}

/// `Etc/UTC`, `Etc/GMT` and `GMT`, and every name linked to them, share the
/// primary identifier `UTC` (`canonicalize-utc-timezone.js`); the other `Etc/`
/// zones are their own primary (`timeZones-include-non-continental.js`).
#[test]
fn the_utc_group_shares_the_primary_identifier_utc() {
    for name in [
        "UTC",
        "Etc/UTC",
        "Etc/GMT",
        "GMT",
        "Etc/GMT+0",
        "Etc/GMT-0",
        "Etc/GMT0",
        "Etc/Greenwich",
        "Etc/UCT",
        "Etc/Universal",
        "Etc/Zulu",
        "GMT+0",
        "GMT-0",
        "GMT0",
        "Greenwich",
        "UCT",
        "Universal",
        "Zulu",
    ] {
        assert_eq!(primary(name), "UTC", "{name}");
    }
    assert!(is_primary_time_zone_identifier("UTC"));
    assert!(!is_primary_time_zone_identifier("Etc/UTC"));
    for name in ["Etc/GMT+1", "Etc/GMT-14", "Factory"] {
        assert_eq!(primary(name), name, "{name}");
    }
}

/// Lookup is ASCII-case-insensitive and reports the tz database's spelling,
/// like `GetAvailableNamedTimeZoneIdentifier`; an unknown name has no record.
#[test]
fn lookup_ignores_case_and_rejects_unknown_names() {
    assert_eq!(primary("asia/calcutta"), "Asia/Kolkata");
    assert_eq!(primary("US/PACIFIC"), "America/Los_Angeles");
    assert_eq!(primary("europe/prague"), "Europe/Prague");
    assert!(!is_primary_time_zone_identifier("asia/calcutta"));
    assert!(is_primary_time_zone_identifier("europe/prague"));
    assert_eq!(primary_time_zone_identifier("Mars/Olympus_Mons"), None);
    assert_eq!(primary_time_zone_identifier(""), None);
    assert_eq!(primary_time_zone_identifier("+01:00"), None);
    assert!(!is_primary_time_zone_identifier("Mars/Olympus_Mons"));
}

/// `Intl.supportedValuesOf("timeZone")` returns the primary identifiers, sorted
/// and unique (`timeZones.js`, `timeZones-include-non-continental.js`,
/// `canonical-not-equal.js`): no alias, every primary, and no two of them
/// equal.
#[test]
fn supported_values_list_exactly_the_primary_identifiers() {
    let zones = supported_values_of("timeZone").unwrap();
    assert!(
        zones.windows(2).all(|pair| pair[0] < pair[1]),
        "sorted and unique"
    );
    assert_eq!(zones.len(), 446);
    for zone in &zones {
        assert_eq!(
            primary(zone),
            zone,
            "{zone} is listed, so it is its own primary"
        );
    }
    for alias in [
        "Asia/Calcutta",
        "US/Pacific",
        "Etc/UTC",
        "Etc/GMT",
        "GMT",
        "Europe/Kiev",
        "CET",
    ] {
        assert!(
            !zones.iter().any(|zone| zone == alias),
            "{alias} is an alias"
        );
    }
    for zone in [
        "UTC",
        "Asia/Kolkata",
        "Africa/Accra",
        "Africa/Abidjan",
        "Etc/GMT+12",
        "Etc/GMT-14",
    ] {
        assert!(zones.iter().any(|listed| listed == zone), "{zone}");
    }
    // Every identifier resolves into the list.
    for identifier in [
        "Asia/Calcutta",
        "Etc/GMT0",
        "Pacific/Truk",
        "Atlantic/Jan_Mayen",
        "PST8PDT",
    ] {
        let resolved = primary(identifier);
        assert!(
            zones.iter().any(|listed| listed == resolved),
            "{identifier} -> {resolved}"
        );
    }
}
