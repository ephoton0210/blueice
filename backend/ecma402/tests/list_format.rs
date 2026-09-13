// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral `Intl.ListFormat` coverage.

use blueice_ecma402::{
    canonicalize, resolve_list_format_locale, supported_list_format_locales, ListFormat,
    ListFormatError, ListFormatOptions, ListPart, ListPartKind, ListStyle, ListType, LocaleMatcher,
    ResolvedListFormatOptions,
};

#[test]
fn formats_conjunction_lists_with_locale_specific_conditionals() {
    let spanish = canonicalize("es").unwrap();
    let formatter = ListFormat::try_new(&[spanish], Default::default()).unwrap();

    assert_eq!(
        formatter.format(["España", "Suiza", "Italia"].iter()),
        "España, Suiza e Italia"
    );
    assert_eq!(
        formatter.resolved_options(),
        &ResolvedListFormatOptions {
            locale: "es".into(),
            list_type: ListType::Conjunction,
            style: ListStyle::Wide,
        }
    );
}

#[test]
fn formats_disjunction_and_unit_lists_with_requested_styles() {
    let thai = canonicalize("th").unwrap();
    let disjunction = ListFormat::try_new(
        &[thai],
        ListFormatOptions {
            list_type: ListType::Disjunction,
            style: ListStyle::Short,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(disjunction.format(["1", "2", "3"].iter()), "1, 2 หรือ 3");

    let english = canonicalize("en").unwrap();
    let units = ListFormat::try_new(
        &[english],
        ListFormatOptions {
            list_type: ListType::Unit,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(units.format(["1ft", "2in"].iter()), "1ft, 2in");
}

#[test]
fn exposes_locale_negotiation_and_default_fallback() {
    let unavailable = canonicalize("zz").unwrap();
    let japanese = canonicalize("ja").unwrap();
    let formatter = ListFormat::try_new(&[unavailable, japanese], Default::default()).unwrap();

    assert_eq!(formatter.negotiation().selected().as_str(), "ja");
    assert!(!formatter.negotiation().used_default());
    assert_eq!(
        formatter
            .negotiation()
            .candidates()
            .iter()
            .map(|candidate| candidate.is_supported())
            .collect::<Vec<_>>(),
        vec![false, true]
    );

    let fallback = ListFormat::try_new(&[canonicalize("zz").unwrap()], Default::default()).unwrap();
    assert_eq!(fallback.negotiation().selected().as_str(), "en-US");
    assert!(fallback.negotiation().used_default());
    assert_eq!(fallback.format(["A", "B", "C"].iter()), "A, B, and C");
}

#[test]
fn exposes_locale_literals_and_input_elements_as_parts() {
    let english = canonicalize("en").unwrap();
    let formatter = ListFormat::try_new(&[english], Default::default()).unwrap();

    assert_eq!(
        formatter.format_to_parts(["A", "B", "C"].iter()).unwrap(),
        vec![
            ListPart {
                kind: ListPartKind::Element,
                value: "A".into(),
            },
            ListPart {
                kind: ListPartKind::Literal,
                value: ", ".into(),
            },
            ListPart {
                kind: ListPartKind::Element,
                value: "B".into(),
            },
            ListPart {
                kind: ListPartKind::Literal,
                value: ", and ".into(),
            },
            ListPart {
                kind: ListPartKind::Element,
                value: "C".into(),
            },
        ]
    );
}

#[test]
fn covers_list_locale_filters_widths_empty_values_and_error_text() {
    let requested = [canonicalize("zz").unwrap(), canonicalize("fr").unwrap()];
    assert_eq!(
        resolve_list_format_locale(&requested, LocaleMatcher::BestFit).as_str(),
        "fr"
    );
    assert_eq!(
        supported_list_format_locales(&requested, LocaleMatcher::BestFit)
            .iter()
            .map(|locale| locale.as_str())
            .collect::<Vec<_>>(),
        ["fr"]
    );
    let trace = blueice_ecma402::negotiate_list_format_locale(&requested, LocaleMatcher::BestFit);
    assert_eq!(
        trace
            .candidates()
            .iter()
            .map(|candidate| candidate.requested().as_str())
            .collect::<Vec<_>>(),
        ["zz", "fr"]
    );
    let narrow = ListFormat::try_new(
        &[canonicalize("en").unwrap()],
        ListFormatOptions {
            locale_matcher: LocaleMatcher::BestFit,
            list_type: ListType::Conjunction,
            style: ListStyle::Narrow,
        },
    )
    .unwrap();
    assert_eq!(narrow.negotiation().matcher(), LocaleMatcher::BestFit);
    assert_eq!(narrow.format(std::iter::empty::<&str>()), "");
    assert_eq!(narrow.format(["only"]), "only");
    assert!(narrow.bytes() > std::mem::size_of::<ListFormat>());
    assert_eq!(
        ListFormatError::DataUnavailable.to_string(),
        "list-pattern data is unavailable"
    );
    assert_eq!(
        ListFormatError::FormattingFailed.to_string(),
        "could not collect list-format parts"
    );
}
