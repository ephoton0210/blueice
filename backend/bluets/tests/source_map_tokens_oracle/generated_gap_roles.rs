// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reviewed generated-only token runs for the six divergent decorator rows.
//!
//! Every source-anchored token is compared in order, with its role, source,
//! UTF-16 position and optional name. The runs below freeze every remaining
//! token's lexical content, count and mapped/unmapped transition. Different
//! helper placement is explicit rather than silently removed from comparison.

use super::{alignment, TokenMapping};

struct ExpectedGap {
    signature: alignment::GeneratedGap,
    role: &'static str,
}

const fn gap(
    after_mapped: usize,
    token_count: usize,
    fingerprint: u64,
    role: &'static str,
) -> ExpectedGap {
    ExpectedGap {
        signature: alignment::GeneratedGap {
            after_mapped,
            token_count,
            fingerprint,
        },
        role,
    }
}

const STANDARD_NATIVE: &[ExpectedGap] = &[
    gap(
        0,
        502,
        16062936152577738884,
        "helper definitions and strict prologue",
    ),
    gap(12, 1, 2661059678015371007, "class declaration scaffold"),
    gap(19, 11, 2747722540138808021, "class IIFE scaffold"),
    gap(21, 27, 4979672264861681666, "decorator setup scaffold"),
    gap(
        79,
        34,
        8662537613671346192,
        "decorator application scaffold",
    ),
    gap(87, 4, 9091175521513516275, "synthetic constructor"),
    gap(94, 8, 10606269605329086372, "class closure scaffold"),
];

const STANDARD_BLUE: &[ExpectedGap] = &[
    gap(
        12,
        485,
        1039267983471646150,
        "relocated helper definitions and class prelude",
    ),
    gap(19, 11, 2747722540138808021, "class IIFE scaffold"),
    gap(21, 27, 4979672264861681666, "decorator setup scaffold"),
    gap(
        79,
        34,
        8662537613671346192,
        "decorator application scaffold",
    ),
    gap(87, 4, 9091175521513516275, "synthetic constructor"),
    gap(94, 8, 10606269605329086372, "class closure scaffold"),
];

const COMPACTED_NATIVE: &[ExpectedGap] = &[
    gap(
        0,
        502,
        16062936152577738884,
        "helper definitions and strict prologue",
    ),
    gap(12, 1, 2661059678015371007, "class declaration scaffold"),
    gap(19, 11, 2747722540138808021, "class IIFE scaffold"),
    gap(21, 27, 4979672264861681666, "decorator setup scaffold"),
    gap(
        81,
        34,
        8662537613671346192,
        "decorator application scaffold",
    ),
    gap(89, 4, 9091175521513516275, "synthetic constructor"),
    gap(96, 8, 10606269605329086372, "class closure scaffold"),
];

const COMPACTED_BLUE: &[ExpectedGap] = &[
    gap(
        12,
        485,
        1039267983471646150,
        "relocated helper definitions and class prelude",
    ),
    gap(19, 11, 2747722540138808021, "class IIFE scaffold"),
    gap(21, 27, 4979672264861681666, "decorator setup scaffold"),
    gap(
        81,
        34,
        8662537613671346192,
        "decorator application scaffold",
    ),
    gap(89, 4, 9091175521513516275, "synthetic constructor"),
    gap(96, 8, 10606269605329086372, "class closure scaffold"),
];

const COMPOSED_NATIVE: &[ExpectedGap] = &[
    gap(
        0,
        502,
        16062936152577738884,
        "helper definitions and strict prologue",
    ),
    gap(57, 1, 2661059678015371007, "class declaration scaffold"),
    gap(64, 4, 2514843842153051197, "class IIFE scaffold"),
    gap(67, 60, 14433495950223052646, "decorator setup scaffold"),
    gap(
        162,
        41,
        13400860980566932026,
        "decorator application scaffold",
    ),
    gap(169, 1, 639450707056825397, "class closure scaffold"),
    gap(177, 4, 9091175521513516275, "synthetic constructor"),
    gap(184, 13, 11619436445773479250, "class closure scaffold"),
];

const COMPOSED_BLUE: &[ExpectedGap] = &[
    gap(
        57,
        485,
        1039267983471646150,
        "relocated helper definitions and class prelude",
    ),
    gap(64, 4, 2514843842153051197, "class IIFE scaffold"),
    gap(67, 79, 8100908024315005547, "decorator setup scaffold"),
    gap(
        162,
        41,
        13400860980566932026,
        "decorator application scaffold",
    ),
    gap(169, 1, 639450707056825397, "class closure scaffold"),
    gap(177, 4, 9091175521513516275, "synthetic constructor"),
    gap(184, 13, 11619436445773479250, "class closure scaffold"),
];

const LEGACY_NATIVE: &[ExpectedGap] = &[
    gap(
        0,
        172,
        18035880685920064360,
        "legacy helper definitions and strict prologue",
    ),
    gap(52, 1, 639450707056825397, "class closure scaffold"),
    gap(
        57,
        10,
        13513088697916774647,
        "legacy member helper-call scaffold",
    ),
    gap(
        64,
        2,
        1839565620924516396,
        "legacy class helper-call scaffold",
    ),
];

const LEGACY_BLUE: &[ExpectedGap] = &[
    gap(
        38,
        162,
        17059140756012342820,
        "relocated legacy helper definitions",
    ),
    gap(52, 1, 639450707056825397, "class closure scaffold"),
    gap(
        57,
        10,
        13513088697916774647,
        "legacy member helper-call scaffold",
    ),
    gap(
        64,
        2,
        1839565620924516396,
        "legacy class helper-call scaffold",
    ),
];

const LEGACY_METADATA_NATIVE: &[ExpectedGap] = &[
    gap(
        0,
        254,
        8521278515063862919,
        "parameter/metadata helper definitions and strict prologue",
    ),
    gap(55, 1, 639450707056825397, "class closure scaffold"),
    gap(
        67,
        32,
        8488282144452698867,
        "parameter/metadata helper-call scaffold",
    ),
];

const LEGACY_METADATA_BLUE: &[ExpectedGap] = &[
    gap(
        41,
        229,
        13896640815062758505,
        "relocated parameter/metadata helper definitions",
    ),
    gap(55, 1, 639450707056825397, "class closure scaffold"),
    gap(
        67,
        32,
        3797060131136808842,
        "parameter/metadata helper-call scaffold",
    ),
];

fn assert_one_profile(
    case: &str,
    compiler: &str,
    tokens: &[TokenMapping],
    mapped_count: usize,
    expected: &[ExpectedGap],
) {
    assert_eq!(
        tokens
            .iter()
            .filter(|token| token.source_index >= 0)
            .count(),
        mapped_count,
        "{case}/{compiler}: source-anchored role count"
    );
    let actual = alignment::generated_gaps(tokens);
    assert_eq!(
        actual.len(),
        expected.len(),
        "{case}/{compiler}: generated-only run count"
    );
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            !expected.role.is_empty(),
            "generated-only role must be named"
        );
        assert_eq!(
            *actual, expected.signature,
            "{case}/{compiler}: {} generated-only tokens and transitions",
            expected.role
        );
    }
}

pub(super) fn assert_decorator_token_roles(
    case: &str,
    native: &[TokenMapping],
    blue: &[TokenMapping],
) {
    let (mapped_count, native_gaps, blue_gaps) = match case {
        "k11-standard-method" | "k11-erased-relocation" => (108, STANDARD_NATIVE, STANDARD_BLUE),
        "k11-compacted-relocation" => (110, COMPACTED_NATIVE, COMPACTED_BLUE),
        "k11-composed-decorators" => (198, COMPOSED_NATIVE, COMPOSED_BLUE),
        "k11-legacy-class-member" => (81, LEGACY_NATIVE, LEGACY_BLUE),
        "k11-legacy-parameter-metadata" => (83, LEGACY_METADATA_NATIVE, LEGACY_METADATA_BLUE),
        _ => panic!("no reviewed decorator role table for {case}"),
    };
    assert_eq!(
        alignment::first_ordered_role_delta(native, blue),
        None,
        "{case}: every source-anchored role, source identity, original UTF-16 position and optional name must match"
    );
    assert_one_profile(case, "Native", native, mapped_count, native_gaps);
    assert_one_profile(case, "BlueTSC", blue, mapped_count, blue_gaps);
}
