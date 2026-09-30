// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral UTF-16 Collator coverage.

use std::cmp::Ordering;

use blueice_ecma402::{
    canonicalize, supports_collation, CaseFirst, Collator, CollatorError, CollatorOptions,
    CollatorUsage, ResolvedCollatorOptions, Sensitivity,
};

#[test]
fn resolves_unicode_extensions_and_compares_numeric_utf16() {
    let locale = canonicalize("en-u-kn-kf-upper").unwrap();
    let collator = Collator::try_new(&[locale], CollatorOptions::default()).unwrap();

    assert_eq!(
        collator.compare_utf16(&['2' as u16], &['1' as u16, '0' as u16]),
        Ordering::Less
    );
    assert_eq!(
        collator.negotiation().selected().as_str(),
        "en-u-kf-upper-kn"
    );
    assert!(!collator.negotiation().used_default());
    assert_eq!(
        collator.resolved_options(),
        &ResolvedCollatorOptions {
            locale: "en-u-kf-upper-kn".into(),
            usage: CollatorUsage::Sort,
            sensitivity: Sensitivity::Variant,
            ignore_punctuation: false,
            collation: "default".into(),
            numeric: true,
            case_first: CaseFirst::Upper,
        }
    );
}

#[test]
fn explicit_options_override_extensions_without_retaining_them_in_the_locale() {
    let locale = canonicalize("en-u-kn-kf-upper").unwrap();
    let collator = Collator::try_new(
        &[locale],
        CollatorOptions {
            numeric: Some(false),
            case_first: Some(CaseFirst::Lower),
            ..Default::default()
        },
    )
    .unwrap();

    let resolved = collator.resolved_options();
    assert_eq!(resolved.locale, "en");
    assert!(!resolved.numeric);
    assert_eq!(resolved.case_first, CaseFirst::Lower);
    assert_eq!(
        collator.compare_utf16(&['2' as u16], &['1' as u16, '0' as u16]),
        Ordering::Greater
    );
}

#[test]
fn search_tailoring_and_unpaired_utf16_are_service_concerns() {
    let german = canonicalize("de-u-co-phonebk").unwrap();
    let collator = Collator::try_new(
        &[german],
        CollatorOptions {
            usage: CollatorUsage::Search,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        collator.resolved_options(),
        &ResolvedCollatorOptions {
            locale: "de".into(),
            usage: CollatorUsage::Search,
            sensitivity: Sensitivity::Variant,
            ignore_punctuation: false,
            collation: "default".into(),
            numeric: false,
            case_first: CaseFirst::False,
        }
    );
    assert_eq!(
        collator.compare_utf16(&['A' as u16, 'E' as u16], &[0x00c4]),
        Ordering::Equal
    );
    assert_eq!(
        collator.compare_utf16(&[0xd800], &[0xd800]),
        Ordering::Equal
    );
}

#[test]
fn exposes_supported_tailorings_option_paths_and_utf16_search_folding() {
    for (locale, collation) in [
        ("en", "emoji"),
        ("en", "eor"),
        ("de", "phonebk"),
        ("es", "trad"),
        ("zh", "pinyin"),
        ("zh", "stroke"),
        ("zh", "unihan"),
        ("zh", "zhuyin"),
        ("ja", "unihan"),
        ("ko", "searchjl"),
        ("si", "dict"),
        ("ar", "compat"),
    ] {
        let locale = canonicalize(locale).unwrap();
        assert!(
            supports_collation(locale.locale(), collation),
            "{collation}"
        );
    }
    assert!(!supports_collation(
        canonicalize("en").unwrap().locale(),
        "phonebk"
    ));

    for (sensitivity, left, right, expected) in [
        (Sensitivity::Base, "a", "A", Ordering::Equal),
        (Sensitivity::Accent, "e", "é", Ordering::Less),
        (Sensitivity::Case, "a", "A", Ordering::Less),
        (Sensitivity::Variant, "a", "A", Ordering::Less),
    ] {
        let collator = Collator::try_new(
            &[canonicalize("en").unwrap()],
            CollatorOptions {
                sensitivity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            collator.compare_utf16(
                &left.encode_utf16().collect::<Vec<_>>(),
                &right.encode_utf16().collect::<Vec<_>>()
            ),
            expected
        );
    }

    let punctuation =
        Collator::try_new(&[canonicalize("th").unwrap()], CollatorOptions::default()).unwrap();
    assert!(punctuation.resolved_options().ignore_punctuation);
    assert!(punctuation.bytes() > std::mem::size_of::<Collator>());

    let tailored = Collator::try_new(
        &[canonicalize("de").unwrap()],
        CollatorOptions {
            collation: Some("phonebk".into()),
            case_first: Some(CaseFirst::Upper),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(tailored.resolved_options().collation, "phonebk");
    assert_eq!(tailored.resolved_options().case_first, CaseFirst::Upper);
    let false_case_first = Collator::try_new(
        &[canonicalize("en").unwrap()],
        CollatorOptions {
            case_first: Some(CaseFirst::False),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        false_case_first.resolved_options().case_first,
        CaseFirst::False
    );

    let german = Collator::try_new(
        &[canonicalize("de").unwrap()],
        CollatorOptions {
            usage: CollatorUsage::Search,
            ..Default::default()
        },
    )
    .unwrap();
    for (source, folded) in [
        ("Ä", "AE"),
        ("Ö", "OE"),
        ("Ü", "UE"),
        ("ß", "ss"),
        ("ä", "ae"),
        ("ö", "oe"),
        ("ü", "ue"),
    ] {
        assert_eq!(
            german.compare_utf16(
                &source.encode_utf16().collect::<Vec<_>>(),
                &folded.encode_utf16().collect::<Vec<_>>()
            ),
            Ordering::Equal,
            "{source}"
        );
    }
    assert_eq!(
        CollatorError::DataUnavailable.to_string(),
        "collation data is unavailable"
    );
}
