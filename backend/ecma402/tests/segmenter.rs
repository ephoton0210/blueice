// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public, host-neutral `Intl.Segmenter` coverage.

use blueice_ecma402::{
    canonicalize, resolve_segmenter_locale, supported_segmenter_locales, LocaleMatcher, Segmenter,
    SegmenterError, SegmenterGranularity, SegmenterOptions, SegmenterSegment,
};

#[test]
fn segments_extended_graphemes_at_utf16_indices() {
    let segmenter = Segmenter::try_new(&[], Default::default()).unwrap();

    assert_eq!(
        segmenter.segment("a🇹🇼e\u{301}"),
        vec![
            SegmenterSegment {
                segment: "a".into(),
                index_utf16: 0,
                is_word_like: None,
            },
            SegmenterSegment {
                segment: "🇹🇼".into(),
                index_utf16: 1,
                is_word_like: None,
            },
            SegmenterSegment {
                segment: "e\u{301}".into(),
                index_utf16: 5,
                is_word_like: None,
            },
        ]
    );
    assert_eq!(segmenter.resolved_options().locale, "en-US");
}

#[test]
fn distinguishes_words_from_punctuation_and_whitespace() {
    let english = canonicalize("en").unwrap();
    let segmenter = Segmenter::try_new(
        &[english],
        SegmenterOptions {
            granularity: SegmenterGranularity::Word,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        segmenter.segment("Hello, 42!"),
        vec![
            SegmenterSegment {
                segment: "Hello".into(),
                index_utf16: 0,
                is_word_like: Some(true),
            },
            SegmenterSegment {
                segment: ",".into(),
                index_utf16: 5,
                is_word_like: Some(false),
            },
            SegmenterSegment {
                segment: " ".into(),
                index_utf16: 6,
                is_word_like: Some(false),
            },
            SegmenterSegment {
                segment: "42".into(),
                index_utf16: 7,
                is_word_like: Some(true),
            },
            SegmenterSegment {
                segment: "!".into(),
                index_utf16: 9,
                is_word_like: Some(false),
            },
        ]
    );
}

#[test]
fn applies_selected_locale_and_sentence_boundaries() {
    let unavailable = canonicalize("zz").unwrap();
    let finnish = canonicalize("fi").unwrap();
    let words = Segmenter::try_new(
        &[unavailable, finnish],
        SegmenterOptions {
            granularity: SegmenterGranularity::Word,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(words.negotiation().selected().as_str(), "fi");
    assert_eq!(words.segment("EU:ssa")[0].segment, "EU:ssa");

    let sentences = Segmenter::try_new(
        &[canonicalize("en").unwrap()],
        SegmenterOptions {
            granularity: SegmenterGranularity::Sentence,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        sentences.segment("One. Two!"),
        vec![
            SegmenterSegment {
                segment: "One. ".into(),
                index_utf16: 0,
                is_word_like: None,
            },
            SegmenterSegment {
                segment: "Two!".into(),
                index_utf16: 5,
                is_word_like: None,
            },
        ]
    );
}

#[test]
fn covers_segmenter_locale_filters_defaults_and_host_storage() {
    let requested = [canonicalize("zz").unwrap(), canonicalize("ja").unwrap()];
    assert_eq!(
        resolve_segmenter_locale(&requested, LocaleMatcher::BestFit).as_str(),
        "ja"
    );
    assert_eq!(
        supported_segmenter_locales(&requested, LocaleMatcher::BestFit)
            .iter()
            .map(|locale| locale.as_str())
            .collect::<Vec<_>>(),
        ["ja"]
    );
    let graphemes = Segmenter::try_new(
        &[canonicalize("zz").unwrap()],
        SegmenterOptions {
            locale_matcher: LocaleMatcher::BestFit,
            granularity: SegmenterGranularity::Grapheme,
        },
    )
    .unwrap();
    assert_eq!(graphemes.resolved_options().locale, "en-US");
    assert_eq!(
        graphemes.resolved_options().granularity,
        SegmenterGranularity::Grapheme
    );
    assert_eq!(graphemes.negotiation().matcher(), LocaleMatcher::BestFit);
    assert!(graphemes.negotiation().used_default());
    assert_eq!(
        graphemes
            .negotiation()
            .candidates()
            .iter()
            .map(|candidate| candidate.is_supported())
            .collect::<Vec<_>>(),
        [false]
    );
    assert_eq!(
        graphemes.negotiation().candidates()[0].requested().as_str(),
        "zz"
    );
    assert!(graphemes.bytes() > std::mem::size_of::<Segmenter>());
    assert_eq!(
        SegmenterError::DataUnavailable.to_string(),
        "segmentation data is unavailable"
    );
}
