// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Failure diagnostics for source-anchored tokens in divergent JavaScript.
//!
//! An equal inventory is not acceptance: K.11 still requires ordered,
//! role-checked alignment and every mapped/unmapped transition. This only
//! identifies the next source-site discrepancy without hiding a red row.

use super::{Mapping, TokenMapping};
use ring::digest::{digest, SHA256};
use std::collections::{BTreeMap, BTreeSet};

/// A decoded, line-reset-aware inventory of every mapped/unmapped state
/// change, including transitions that occur in trivia between lexer tokens.
/// Generated coordinates are part of the digest; raw VLQ text is not.
pub(super) fn transition_fingerprint(mappings: &BTreeSet<Mapping>) -> (usize, String) {
    let mut bytes = Vec::new();
    let mut line = None;
    let mut mapped = false;
    let mut count = 0;
    for segment in mappings {
        if line != Some(segment.generated_line) {
            line = Some(segment.generated_line);
            mapped = false;
        }
        let next = segment.source_index >= 0;
        if mapped == next {
            continue;
        }
        bytes.extend_from_slice(&(segment.generated_line as u64).to_le_bytes());
        bytes.extend_from_slice(&segment.generated_column.to_le_bytes());
        bytes.push(u8::from(next));
        mapped = next;
        count += 1;
    }
    let hash = digest(&SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    (count, hash)
}

/// Pinned TypeScript 5.9.3 and current BlueTSC decoded-state schedules for
/// the frozen public matrix. These include state changes in copied comments
/// and whitespace that a non-trivia token inventory cannot observe. A
/// divergent generated layout has a separate checked schedule, not a claim
/// that its generated coordinates equal TypeScript's.
type TransitionCountHash = (usize, &'static str);
type TransitionBaseline = (&'static str, TransitionCountHash, TransitionCountHash);

const TRANSITIONS: &[TransitionBaseline] = &[
    (
        "k11-basic-copy",
        (
            6,
            "2195a23867fd30def4c6bfe2870ef1cf3138eb96f648b6ea5525acfc7abed7d5",
        ),
        (
            6,
            "2195a23867fd30def4c6bfe2870ef1cf3138eb96f648b6ea5525acfc7abed7d5",
        ),
    ),
    (
        "k11-erased-annotation",
        (
            6,
            "2195a23867fd30def4c6bfe2870ef1cf3138eb96f648b6ea5525acfc7abed7d5",
        ),
        (
            6,
            "2195a23867fd30def4c6bfe2870ef1cf3138eb96f648b6ea5525acfc7abed7d5",
        ),
    ),
    (
        "k11-standard-method",
        (
            11,
            "341069ae168036f40442033c5d3f8e3568756a923bd09994672b04be20faf4e1",
        ),
        (
            17,
            "176ca4d8fdcef203dc06f4d1426180f7f670fab536ebc81cd9b550c5a9097369",
        ),
    ),
    (
        "k11-compacted-relocation",
        (
            11,
            "341069ae168036f40442033c5d3f8e3568756a923bd09994672b04be20faf4e1",
        ),
        (
            17,
            "827d6b4c767918dc3414380b75fe5728aed33cbda4dc3a46b81601453780e0ec",
        ),
    ),
    (
        "k11-erased-relocation",
        (
            11,
            "341069ae168036f40442033c5d3f8e3568756a923bd09994672b04be20faf4e1",
        ),
        (
            18,
            "eeee8936810a12c148fd91aa257078560c1f40421d7d1b00fa68aaea599f1364",
        ),
    ),
    (
        "k11-composed-decorators",
        (
            15,
            "1fbbb75c6bff83752585f7424b0a0d0cd1e681d5981b953166391118deb60145",
        ),
        (
            23,
            "16267cbfc85626593465b06506bce6f5785339355da43b69de72bec5f10cec39",
        ),
    ),
    (
        "k11-legacy-class-member",
        (
            15,
            "15c0c76bac994fe9d18fb69fd6b9203fe046c1ae2af3878d851a8f497ae87a5f",
        ),
        (
            18,
            "e848edb6524df7b0a7795c806947c7dcb2bbbfb4eaaf37a64590e246889e36ff",
        ),
    ),
    (
        "k11-legacy-parameter-metadata",
        (
            18,
            "659c0d5541f25df174b141ce0f143cdb81e47d859ac4dc03a919a02b43d97572",
        ),
        (
            17,
            "b26b57ebbcb2d92e8d7313ab6c2dd13424e8e9bc500aad70d2152b27553d931b",
        ),
    ),
    (
        "k11-nondecorator-helper",
        (
            6,
            "34f7bbba53058987fd4d5b1b4b12cd525918cbf03066757f7971b0295e110981",
        ),
        (
            10,
            "2ede1d8a7627a79fdc5fcebcd10f88f40213fc345b913ee6b80e168479792a4b",
        ),
    ),
    (
        "k11-system-wrapper",
        (
            7,
            "42fa2c1f7761d31efa18a99184687351290912e4b2f7def536fb734b3d07fb1d",
        ),
        (
            8,
            "3efc8be9b1ceafc6ae5e093ee3a18d223d42db50829e54db4a8eefcc35a52c6e",
        ),
    ),
    (
        "k11-crlf-astral",
        (
            5,
            "5e440aaf88e8546c0b4f2a84ec57fefe39e53651cddff172a429e7f24d4ba580",
        ),
        (
            6,
            "2195a23867fd30def4c6bfe2870ef1cf3138eb96f648b6ea5525acfc7abed7d5",
        ),
    ),
];

pub(super) fn assert_transition_inventory(
    case: &str,
    native: &BTreeSet<Mapping>,
    blue: &BTreeSet<Mapping>,
) {
    let (_, native_expected, blue_expected) = TRANSITIONS
        .iter()
        .find(|(name, _, _)| *name == case)
        .expect("every frozen K.11 matrix row needs a transition inventory");
    let native_actual = transition_fingerprint(native);
    let blue_actual = transition_fingerprint(blue);
    assert_eq!(
        (native_actual.0, native_actual.1.as_str()),
        *native_expected,
        "{case}: every Native decoded mapped/unmapped transition, including trivia"
    );
    assert_eq!(
        (blue_actual.0, blue_actual.1.as_str()),
        *blue_expected,
        "{case}: every BlueTSC decoded mapped/unmapped transition, including trivia"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GeneratedGap {
    pub(super) after_mapped: usize,
    pub(super) token_count: usize,
    pub(super) fingerprint: u64,
}

/// Nonempty generated-only runs, indexed by the mapped token immediately
/// before the run. The stable FNV-1a fingerprint includes every token spelling
/// and a separator, so a role table can freeze the complete token stream
/// without depending on helper source line wrapping or raw VLQ bytes.
pub(super) fn generated_gaps(tokens: &[TokenMapping]) -> Vec<GeneratedGap> {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut gaps = Vec::new();
    let mut after_mapped = 0;
    let mut count = 0;
    let mut fingerprint = OFFSET;
    for token in tokens {
        if token.source_index < 0 {
            count += 1;
            for byte in token.token.bytes().chain(std::iter::once(0xff)) {
                fingerprint = (fingerprint ^ u64::from(byte)).wrapping_mul(PRIME);
            }
        } else {
            if count != 0 {
                gaps.push(GeneratedGap {
                    after_mapped,
                    token_count: count,
                    fingerprint,
                });
                count = 0;
                fingerprint = OFFSET;
            }
            after_mapped += 1;
        }
    }
    if count != 0 {
        gaps.push(GeneratedGap {
            after_mapped,
            token_count: count,
            fingerprint,
        });
    }
    gaps
}

fn role(token: &str) -> &str {
    match token {
        "__esDecorate" | "__bluetsEsDecorate" => "<es-decorate-call>",
        "__runInitializers" | "__bluetsRunInitializers" => "<run-initializers-call>",
        "__decorate" | "__bluetsDecorate" => "<legacy-decorate-call>",
        "__param" | "__bluetsParam" => "<legacy-param-call>",
        "__metadata" | "__bluetsMetadata" => "<legacy-metadata-call>",
        "__assign" | "__blueice_target_object_properties_0" => "<object-assign-call>",
        _ => token,
    }
}

fn mapped_roles(tokens: &[TokenMapping]) -> Vec<(i64, i64, i64, Option<i64>, String)> {
    tokens
        .iter()
        .filter(|token| token.source_index >= 0)
        .map(|token| {
            (
                token.source_index,
                token.source_line,
                token.source_column,
                token.name_index,
                role(&token.token).to_string(),
            )
        })
        .collect()
}

pub(super) fn first_ordered_role_delta(
    native: &[TokenMapping],
    blue: &[TokenMapping],
) -> Option<String> {
    let native = mapped_roles(native);
    let blue = mapped_roles(blue);
    let index = native.iter().zip(&blue).take_while(|(a, b)| a == b).count();
    if index == native.len() && index == blue.len() {
        None
    } else {
        Some(format!(
            "index {index} of Native {} / BlueTSC {}: Native {:?}, BlueTSC {:?}",
            native.len(),
            blue.len(),
            &native[index.min(native.len())..native.len().min(index + 4)],
            &blue[index.min(blue.len())..blue.len().min(index + 4)]
        ))
    }
}

/// A divergent emission can have different generated helper text, but its
/// source-anchored tokens must be separated by the same mapped/unmapped
/// transitions. Every token in a nonempty gap is generated-only; Native's
/// decoded map is the expected unmapped state for the corresponding Blue gap.
pub(super) fn first_transition_delta(
    native: &[TokenMapping],
    blue: &[TokenMapping],
) -> Option<String> {
    let gaps = |tokens: &[TokenMapping]| {
        let mut gaps = Vec::new();
        let mut pending = 0;
        for token in tokens {
            if token.source_index < 0 {
                pending += 1;
            } else {
                gaps.push(pending);
                pending = 0;
            }
        }
        gaps.push(pending);
        gaps
    };
    let native = gaps(native);
    let blue = gaps(blue);
    let index = native
        .iter()
        .zip(&blue)
        .take_while(|(native, blue)| (**native == 0) == (**blue == 0))
        .count();
    if index == native.len() && index == blue.len() {
        None
    } else {
        Some(format!(
            "gap {index} of Native {} / BlueTSC {}: Native {:?} generated-only tokens, BlueTSC {:?}",
            native.len(),
            blue.len(),
            native.get(index),
            blue.get(index)
        ))
    }
}

pub(super) fn first_mapped_role_deltas(
    native: &[TokenMapping],
    blue: &[TokenMapping],
) -> Vec<String> {
    let mut counts = BTreeMap::<(i64, i64, i64, Option<i64>, String), (usize, usize)>::new();
    for (side, tokens) in [(0, native), (1, blue)] {
        for token in tokens.iter().filter(|token| token.source_index >= 0) {
            let key = (
                token.source_index,
                token.source_line,
                token.source_column,
                token.name_index,
                role(&token.token).to_string(),
            );
            let entry = counts.entry(key).or_default();
            if side == 0 {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }
    counts
        .into_iter()
        .filter(|(_, (native, blue))| native != blue)
        .take(8)
        .map(|((_, line, column, name, role), (native, blue))| {
            format!("{line}:{column} {role} name={name:?} Native={native} BlueTSC={blue}")
        })
        .collect()
}

#[test]
fn known_helper_names_share_only_their_explicit_roles() {
    let site = |token: &str| TokenMapping {
        token: token.to_string(),
        source_index: 0,
        source_line: 2,
        source_column: 4,
        name_index: None,
    };
    assert!(
        first_mapped_role_deltas(&[site("__esDecorate")], &[site("__bluetsEsDecorate")]).is_empty()
    );
    assert!(first_mapped_role_deltas(
        &[site("__assign")],
        &[site("__blueice_target_object_properties_0")]
    )
    .is_empty());
    assert_eq!(
        first_mapped_role_deltas(&[site("user")], &[site("synthetic")]).len(),
        2,
        "unknown tokens cannot be silently equated"
    );
    assert!(
        first_ordered_role_delta(&[site("__esDecorate")], &[site("__bluetsEsDecorate")]).is_none()
    );
    assert!(first_ordered_role_delta(&[site("user")], &[site("synthetic")]).is_some());
}

#[test]
fn divergent_generated_text_must_preserve_every_mapping_transition() {
    let mapped = |token: &str| TokenMapping {
        token: token.to_string(),
        source_index: 0,
        source_line: 1,
        source_column: 2,
        name_index: None,
    };
    let unmapped = |token: &str| TokenMapping {
        source_index: -1,
        ..mapped(token)
    };
    assert!(first_transition_delta(
        &[unmapped("native-helper"), mapped("x")],
        &[unmapped("blue"), unmapped("helper"), mapped("x")]
    )
    .is_none());
    assert!(first_transition_delta(&[unmapped("helper"), mapped("x")], &[mapped("x")]).is_some());
}

#[test]
fn generated_gap_signature_covers_every_unmapped_token_and_transition() {
    let token = |text: &str, mapped: bool| TokenMapping {
        token: text.to_string(),
        source_index: if mapped { 0 } else { -1 },
        source_line: 0,
        source_column: 0,
        name_index: None,
    };
    let baseline = [
        token("helper", false),
        token("a", true),
        token("flag", false),
    ];
    let gaps = generated_gaps(&baseline);
    assert_eq!(gaps.len(), 2);
    assert_eq!((gaps[0].after_mapped, gaps[0].token_count), (0, 1));
    assert_eq!((gaps[1].after_mapped, gaps[1].token_count), (1, 1));
    assert_ne!(
        gaps,
        generated_gaps(&[
            token("helper", false),
            token("a", true),
            token("other", false)
        ])
    );
    assert_ne!(
        gaps,
        generated_gaps(&[
            token("a", true),
            token("helper", false),
            token("flag", false)
        ])
    );
}

#[test]
fn transition_fingerprint_includes_trivia_islands_and_line_restarts() {
    let mapping = |line, column, mapped| Mapping {
        generated_line: line,
        generated_column: column,
        source_index: if mapped { 0 } else { -1 },
        source_line: 0,
        source_column: 0,
        name_index: None,
    };
    let baseline = BTreeSet::from([
        mapping(0, 0, true),
        mapping(0, 5, false),
        mapping(0, 7, true),
        mapping(1, 0, true),
    ]);
    let without_island = BTreeSet::from([mapping(0, 0, true), mapping(1, 0, true)]);
    assert_eq!(transition_fingerprint(&baseline).0, 4);
    assert_eq!(transition_fingerprint(&without_island).0, 2);
    assert_ne!(
        transition_fingerprint(&baseline),
        transition_fingerprint(&without_island)
    );
}
