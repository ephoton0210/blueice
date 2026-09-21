// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A zero UTC offset is written with the locale's zero-offset GMT string
//! (`gmtZeroFormat`: "GMT" in English), never as a signed zero such as
//! "GMT+0".
//!
//! ICU4X's localized-offset formatter drops CLDR's `gmtZeroFormat` (its data
//! model deserializes and discards `offset_zero`) and renders the zero offset
//! through the ordinary `gmtFormat` pattern, so `timeZone: "+00:00"` printed
//! `GMT+0` and `timeZoneName: "shortOffset"` for UTC did too, where Test262's
//! `intl402/Temporal/ZonedDateTime/prototype/toLocaleString/offset-time-zones.js`
//! (and every other engine) expects `GMT`.
//!
//! The expected strings are CLDR's own (`cldr-json` `timeZoneNames.gmtFormat`
//! and `gmtZeroFormat`, at the commit CI pins): for 761 of the 766 locales in
//! `cldr-dates-full` the zero string is `gmtFormat` without `{0}` and without
//! the whitespace or bidirectional marks that sat next to the placeholder.
//! `dz`, `ks`, `ks-Arab`, `tok` and `xnr` translate the zero string
//! independently of `gmtFormat`; ICU4X's data no longer carries it, so they
//! get the derived string, which the last test pins as a known limitation.
//! (The runtime patterns come from ICU4X's compiled data, not from `cldr-json`
//! itself; the derivation was checked against `cldr-json`, the assertions
//! below against what the compiled data actually formats.)

use blueice_ecma402::{canonicalize, DateTimeFormat, DateTimeFormatOptions, DateTimeWidth};

/// The value of the `timeZoneName` part of `1970-01-01T00:00Z` formatted in
/// `zone` with `style`.
fn zone_name(locale: &str, zone: &str, style: &str) -> String {
    let format = DateTimeFormat::try_new(
        &[canonicalize(locale).unwrap()],
        DateTimeFormatOptions {
            hour: Some(DateTimeWidth::Numeric),
            time_zone: Some(zone.into()),
            time_zone_name: Some(style.into()),
            ..Default::default()
        },
    )
    .unwrap();
    let parts = format.format_to_parts(0.0).unwrap();
    parts
        .iter()
        .find(|part| part.kind == "timeZoneName")
        .unwrap_or_else(|| panic!("no timeZoneName part for {locale} {zone} {style}: {parts:?}"))
        .value
        .clone()
}

#[test]
fn a_fixed_zero_offset_is_gmt_not_a_signed_zero() {
    for style in ["short", "long", "shortOffset", "longOffset"] {
        assert_eq!(zone_name("en", "+00:00", style), "GMT", "en +00:00 {style}");
    }
}

#[test]
fn utc_in_an_offset_style_is_gmt() {
    assert_eq!(zone_name("en", "UTC", "shortOffset"), "GMT");
    assert_eq!(zone_name("en", "UTC", "longOffset"), "GMT");
}

#[test]
fn the_zero_string_is_the_whole_formatted_text_around_it() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("en").unwrap()],
        DateTimeFormatOptions {
            hour: Some(DateTimeWidth::Numeric),
            time_zone: Some("+00:00".into()),
            time_zone_name: Some("short".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(format.format(0.0).unwrap(), "12 AM GMT");
}

#[test]
fn named_zones_and_non_zero_offsets_are_unchanged() {
    // A real name is not a localized offset, so nothing is rewritten.
    assert_eq!(zone_name("en", "UTC", "short"), "UTC");
    assert_eq!(zone_name("en", "Etc/GMT", "short"), "GMT");
    // Non-zero offsets keep their signed, minimal or full, form.
    assert_eq!(zone_name("en", "+01:00", "short"), "GMT+1");
    assert_eq!(zone_name("en", "-01:00", "short"), "GMT-1");
    assert_eq!(zone_name("en", "+05:30", "short"), "GMT+5:30");
    assert_eq!(zone_name("en", "+01:00", "longOffset"), "GMT+01:00");
    assert_eq!(zone_name("en", "-08:00", "longOffset"), "GMT-08:00");
    assert_eq!(zone_name("en", "Asia/Kolkata", "shortOffset"), "GMT+5:30");
}

/// `zone_name` at an arbitrary instant.
fn zone_name_at(locale: &str, zone: &str, style: &str, epoch_milliseconds: f64) -> String {
    let format = DateTimeFormat::try_new(
        &[canonicalize(locale).unwrap()],
        DateTimeFormatOptions {
            hour: Some(DateTimeWidth::Numeric),
            time_zone: Some(zone.into()),
            time_zone_name: Some(style.into()),
            ..Default::default()
        },
    )
    .unwrap();
    format
        .format_to_parts(epoch_milliseconds)
        .unwrap()
        .into_iter()
        .find(|part| part.kind == "timeZoneName")
        .unwrap()
        .value
}

/// The offset in effect at the instant decides, not the zone: London is at
/// +00:00 in January and +01:00 in July. (At the epoch itself it is +01:00,
/// British Standard Time having run all year from 1968 to 1971.)
#[test]
fn a_named_zone_is_rewritten_only_while_its_offset_is_zero() {
    const JAN_2020: f64 = 1_577_836_800_000.0;
    const JUL_2020: f64 = 1_593_561_600_000.0;
    assert_eq!(
        zone_name_at("en", "Europe/London", "shortOffset", JAN_2020),
        "GMT"
    );
    assert_eq!(
        zone_name_at("en", "Europe/London", "shortOffset", JUL_2020),
        "GMT+1"
    );
    assert_eq!(
        zone_name_at("en", "Europe/London", "longOffset", JAN_2020),
        "GMT"
    );
    assert_eq!(
        zone_name_at("en", "Europe/London", "longOffset", JUL_2020),
        "GMT+01:00"
    );
    assert_eq!(
        zone_name_at("en", "Europe/London", "shortOffset", 0.0),
        "GMT+1"
    );
}

/// A range with a zone name prints it at both ends (ICU4X's interval formatter
/// falls back to the two full endpoints), and each is rewritten.
#[test]
fn range_formatting_rewrites_a_zero_offset_zone_name_too() {
    let format = DateTimeFormat::try_new(
        &[canonicalize("en").unwrap()],
        DateTimeFormatOptions {
            hour: Some(DateTimeWidth::Numeric),
            time_zone: Some("+00:00".into()),
            time_zone_name: Some("shortOffset".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let parts = format.format_range_to_parts(0.0, 3_600_000.0).unwrap();
    let names: Vec<&str> = parts
        .iter()
        .filter(|part| part.kind == "timeZoneName")
        .map(|part| part.value.as_str())
        .collect();
    assert!(!names.is_empty(), "{parts:?}");
    assert!(names.iter().all(|name| *name == "GMT"), "{parts:?}");
    assert_eq!(
        format.format_range(0.0, 3_600_000.0).unwrap(),
        "12 AM GMT\u{2009}\u{2013}\u{2009}1 AM GMT"
    );
}

/// Locales whose `gmtFormat` puts the placeholder next to a space, a
/// bidirectional mark or nothing, on either side of the text.
#[test]
fn the_zero_string_follows_the_locales_own_gmt_format() {
    for (locale, expected) in [
        // `GMT{0}`
        ("en", "GMT"),
        ("de", "GMT"),
        ("ja", "GMT"),
        // `UTC{0}`
        ("fr", "UTC"),
        // `GMT {0}`: the space beside the placeholder is not part of it.
        ("bn", "GMT"),
        ("sl", "GMT"),
        // `{0} GMT`
        ("ee", "GMT"),
        // `GMT{0}` followed by a left-to-right mark.
        ("he", "GMT"),
    ] {
        assert_eq!(
            zone_name(locale, "+00:00", "shortOffset"),
            expected,
            "{locale}"
        );
    }
}

/// CLDR translates `gmtZeroFormat` separately from `gmtFormat` for five locales
/// (`dz`, `ks`, `ks-Arab`, `tok`, `xnr`); Kashmiri writes its own "GMT" in
/// Perso-Arabic script. ICU4X keeps no zero string, so the text derived from
/// `gmtFormat` is used, which is "GMT" here. This records that limitation
/// rather than a desired result. (Of the five, only `ks` and `ks-Arab` carry a
/// distinct `gmtFormat` in ICU4X's compiled data; the others fall back to the
/// root pattern there, so they cannot pin anything.)
#[test]
fn a_locale_whose_zero_string_is_translated_independently_uses_the_derived_one() {
    assert_eq!(zone_name("ks", "+00:00", "shortOffset"), "GMT");
    assert_eq!(zone_name("ks-Arab", "+00:00", "shortOffset"), "GMT");
}
