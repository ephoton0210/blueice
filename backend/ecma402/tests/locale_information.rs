// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral `Intl.Locale` information coverage.

use blueice_ecma402::{
    apply_locale_options, canonicalize, locale_information, maximize_locale, minimize_locale,
    LocaleError, LocaleOptionError, LocaleOptions, TextDirection,
};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn locale_information_uses_unicode_extensions_before_locale_defaults() {
    let locale = canonicalize("ar-TW-u-ca-buddhist-co-phonebk-fw-sun-hc-h24-nu-thai").unwrap();
    let information = locale_information(&locale);

    assert_eq!(information.calendars, strings(&["buddhist"]));
    assert_eq!(information.collations, strings(&["phonebk"]));
    assert_eq!(information.hour_cycles, strings(&["h24"]));
    assert_eq!(information.numbering_systems, strings(&["thai"]));
    assert_eq!(information.text_direction, TextDirection::RightToLeft);
    assert_eq!(information.time_zones, Some(strings(&["Asia/Taipei"])));
    assert_eq!(information.week_info.first_day, 7);
    assert_eq!(information.week_info.weekend, vec![6, 7]);
}

#[test]
fn locale_information_supplies_deterministic_defaults_and_region_data() {
    let us = locale_information(&canonicalize("en-US").unwrap());
    assert_eq!(us.calendars, strings(&["gregory"]));
    assert_eq!(us.collations, strings(&["emoji"]));
    assert_eq!(us.hour_cycles, strings(&["h12"]));
    assert_eq!(us.numbering_systems, strings(&["latn"]));
    assert_eq!(us.text_direction, TextDirection::LeftToRight);
    assert_eq!(
        us.time_zones,
        Some(strings(&[
            "America/Adak",
            "America/Anchorage",
            "America/Boise",
            "America/Chicago",
            "America/Denver",
            "America/Detroit",
            "America/Indiana/Indianapolis",
            "America/Los_Angeles",
            "America/New_York",
            "Pacific/Honolulu",
        ]))
    );
    assert_eq!(us.week_info.first_day, 7);

    let unregional = locale_information(&canonicalize("en").unwrap());
    assert_eq!(unregional.time_zones, None);
    assert_eq!(
        unregional.week_info.first_day, 7,
        "week information uses en's likely US region"
    );
}

#[test]
fn likely_subtag_transforms_preserve_the_host_neutral_canonical_form() {
    let zh = canonicalize("zh").unwrap();
    assert_eq!(maximize_locale(&zh).as_str(), "zh-Hans-CN");

    let expanded = canonicalize("zh-Hans-CN-u-ca-buddhist").unwrap();
    assert_eq!(minimize_locale(&expanded).as_str(), "zh-u-ca-buddhist");

    let posix = canonicalize("posix").unwrap();
    assert_eq!(maximize_locale(&posix).as_str(), "posix");
    assert_eq!(minimize_locale(&posix).as_str(), "posix");
}

#[test]
fn locale_options_are_applied_and_canonicalized_without_a_realm() {
    let locale = apply_locale_options(
        &canonicalize("de").unwrap(),
        &LocaleOptions {
            language: Some("fr".into()),
            script: Some("Latn".into()),
            region: Some("CA".into()),
            variants: Some("fonipa-1901".into()),
            calendar: Some("islamicc".into()),
            collation: Some("phonebk".into()),
            hour_cycle: Some("h23".into()),
            case_first: Some("upper".into()),
            numeric: Some(true),
            numbering_system: Some("latn".into()),
            first_day_of_week: Some("1".into()),
        },
    )
    .unwrap();
    assert_eq!(
        locale.as_str(),
        "fr-Latn-CA-1901-fonipa-u-ca-islamic-civil-co-phonebk-fw-mon-hc-h23-kf-upper-kn-nu-latn"
    );

    assert_eq!(
        apply_locale_options(
            &canonicalize("en").unwrap(),
            &LocaleOptions {
                language: Some("abcd".into()),
                ..Default::default()
            },
        ),
        Err(LocaleOptionError::InvalidLanguage)
    );
    assert_eq!(
        apply_locale_options(
            &canonicalize("en").unwrap(),
            &LocaleOptions {
                first_day_of_week: Some("mo".into()),
                ..Default::default()
            },
        ),
        Err(LocaleOptionError::InvalidFirstDayOfWeek)
    );
}

#[test]
fn locale_option_errors_aliases_and_information_fallbacks_are_explicit() {
    assert_eq!(
        canonicalize("e"),
        Err(LocaleError::InvalidLanguageTag),
        "a one-character primary language subtag is structurally invalid"
    );
    assert_eq!(
        canonicalize("en_US").unwrap_err().to_string(),
        "invalid Unicode locale identifier"
    );
    assert_eq!(
        LocaleError::InvalidLanguageTag.to_string(),
        "invalid Unicode locale identifier"
    );
    assert_eq!(
        LocaleOptionError::InvalidLanguage.to_string(),
        "invalid language option"
    );

    let base = canonicalize("en").unwrap();
    for (options, expected) in [
        (
            LocaleOptions {
                script: Some("Lat".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidScript,
        ),
        (
            LocaleOptions {
                region: Some("U".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidRegion,
        ),
        (
            LocaleOptions {
                variants: Some("".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidVariants,
        ),
        (
            LocaleOptions {
                variants: Some("fonipa-fonipa".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidVariants,
        ),
        (
            LocaleOptions {
                calendar: Some("ab".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidCalendar,
        ),
        (
            LocaleOptions {
                collation: Some("ab".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidCollation,
        ),
        (
            LocaleOptions {
                hour_cycle: Some("h25".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidHourCycle,
        ),
        (
            LocaleOptions {
                case_first: Some("mixed".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidCaseFirst,
        ),
        (
            LocaleOptions {
                numbering_system: Some("ab".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidNumberingSystem,
        ),
        (
            LocaleOptions {
                first_day_of_week: Some("mo".into()),
                ..Default::default()
            },
            LocaleOptionError::InvalidFirstDayOfWeek,
        ),
    ] {
        assert_eq!(apply_locale_options(&base, &options), Err(expected));
        assert_eq!(
            expected.to_string(),
            format!(
                "invalid {} option",
                match expected {
                    LocaleOptionError::InvalidLanguage => "language",
                    LocaleOptionError::InvalidScript => "script",
                    LocaleOptionError::InvalidRegion => "region",
                    LocaleOptionError::InvalidVariants => "variants",
                    LocaleOptionError::InvalidCalendar => "calendar",
                    LocaleOptionError::InvalidCollation => "collation",
                    LocaleOptionError::InvalidHourCycle => "hourCycle",
                    LocaleOptionError::InvalidCaseFirst => "caseFirst",
                    LocaleOptionError::InvalidNumberingSystem => "numberingSystem",
                    LocaleOptionError::InvalidFirstDayOfWeek => "firstDayOfWeek",
                }
            )
        );
    }

    let aliases = canonicalize("en-u-ks-tertiary-ms-imperial-tz-eire").unwrap();
    assert_eq!(aliases.as_str(), "en-u-ks-level3-ms-uksystem-tz-iedub");
    let boolean = canonicalize("en-u-kn-yes").unwrap();
    assert_eq!(boolean.as_str(), "en-u-kn");
    let boolean = canonicalize("en-u-kb-yes").unwrap();
    assert_eq!(boolean.as_str(), "en-u-kb");
    assert_eq!(
        apply_locale_options(&canonicalize("posix").unwrap(), &LocaleOptions::default())
            .unwrap()
            .as_str(),
        "posix"
    );
    let rebuilt = canonicalize("fr-CA").unwrap();
    let (data_locale, canonical) = rebuilt.into_parts();
    assert_eq!(canonical, "fr-CA");
    assert_eq!(data_locale.to_string(), "fr-CA");
    let applied = apply_locale_options(
        &canonicalize("en").unwrap(),
        &LocaleOptions {
            hour_cycle: Some("h11".into()),
            case_first: Some("false".into()),
            numeric: Some(false),
            first_day_of_week: Some("7".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(applied.as_str(), "en-u-fw-sun-hc-h11-kf-false-kn-false");
    assert_eq!(
        canonicalize("en-u-tz-est").unwrap().as_str(),
        "en-u-tz-papty"
    );
    assert_eq!(
        canonicalize("en-u-tz-gmt0").unwrap().as_str(),
        "en-u-tz-gmt"
    );

    for (tag, first_day, time_zone, direction) in [
        ("en-u-fw-tue", 2, None, TextDirection::LeftToRight),
        ("en-u-fw-wed", 3, None, TextDirection::LeftToRight),
        ("en-u-fw-thu", 4, None, TextDirection::LeftToRight),
        ("en-u-fw-fri", 5, None, TextDirection::LeftToRight),
        ("en-u-fw-sat", 6, None, TextDirection::LeftToRight),
        (
            "en-GB",
            1,
            Some("Europe/London"),
            TextDirection::LeftToRight,
        ),
        ("ja-JP", 7, Some("Asia/Tokyo"), TextDirection::LeftToRight),
        ("de-DE", 1, Some("Etc/UTC"), TextDirection::LeftToRight),
        ("en-Arab", 7, None, TextDirection::RightToLeft),
        ("ar", 1, None, TextDirection::RightToLeft),
    ] {
        let information = locale_information(&canonicalize(tag).unwrap());
        assert_eq!(information.week_info.first_day, first_day, "{tag}");
        assert_eq!(
            information
                .time_zones
                .as_ref()
                .and_then(|zones| zones.first())
                .map(String::as_str),
            time_zone,
            "{tag}"
        );
        assert_eq!(information.text_direction, direction, "{tag}");
    }
}

#[test]
fn locale_information_uses_region_preference_for_locale_information_methods() {
    let calendars = locale_information(&canonicalize("en-US-u-rg-thzzzz").unwrap());
    assert_eq!(calendars.calendars, strings(&["buddhist", "gregory"]));

    let hour_cycles = locale_information(&canonicalize("en-u-sd-gbeng").unwrap());
    assert_eq!(hour_cycles.hour_cycles, strings(&["h23"]));

    let language_hour_cycle = locale_information(&canonicalize("en-CA").unwrap());
    assert_eq!(language_hour_cycle.hour_cycles, strings(&["h12"]));

    let week_info = locale_information(&canonicalize("fa-JP-u-sd-inka-rg-afzzzz").unwrap());
    assert_eq!(week_info.week_info.first_day, 6);
    assert_eq!(week_info.week_info.weekend, vec![4, 5]);

    let likely = locale_information(&canonicalize("fa").unwrap());
    assert_eq!(likely.calendars[0], "persian");
    assert_eq!(likely.week_info.first_day, 6);

    let world = locale_information(&canonicalize("eo").unwrap());
    assert_eq!(world.calendars, strings(&["gregory"]));
    assert_eq!(world.hour_cycles, strings(&["h23"]));
    assert_eq!(world.week_info.first_day, 1);
}

#[test]
fn locale_information_uses_root_collations_for_unavailable_locales() {
    for tag in ["und", "und-US", "qfz", "qga-DE", "qtz-CN"] {
        assert_eq!(
            locale_information(&canonicalize(tag).unwrap()).collations,
            strings(&["emoji", "eor"]),
            "{tag}"
        );
    }
}
