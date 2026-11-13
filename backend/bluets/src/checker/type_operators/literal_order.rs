// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned default-library cache order affects literal-union presentation.

use crate::parser::Type;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(serde::Deserialize)]
struct Observation {
    literals: Vec<f64>,
}

/// Cached numeric literals precede new source literals. New literals retain
/// source order, and numeric spelling compares by value as in the native cache.
pub(super) fn numeric_rank(text: &str) -> usize {
    static ORDER: OnceLock<HashMap<u64, usize>> = OnceLock::new();
    let order = ORDER.get_or_init(|| {
        let observation: Observation =
            serde_json::from_str(include_str!("numeric-literal-order.json"))
                .expect("bundled TypeScript 5.9.3 numeric cache observations");
        observation
            .literals
            .into_iter()
            .enumerate()
            .map(|(rank, value)| (value.to_bits(), rank))
            .collect()
    });
    text.parse::<f64>()
        .ok()
        .and_then(|value| order.get(&value.to_bits()).copied())
        .unwrap_or(usize::MAX)
}

#[derive(serde::Deserialize)]
struct StringCatalog {
    schema: u8,
    typescript: String,
    profiles: Vec<StringProfile>,
}

#[derive(serde::Deserialize)]
struct StringProfile {
    target: String,
    mode: String,
    values: Vec<serde_json::Value>,
}

/// Intrinsic mapping allocates output literals in the input union's native
/// order. The selected library's cache precedes new source literals; stable
/// sorting keeps the latter in their original allocation order.
pub(super) fn order_strings(parts: &mut [Type], profile: &str) {
    static PROFILES: OnceLock<HashMap<String, HashMap<Vec<u16>, usize>>> = OnceLock::new();
    if !parts.iter().all(|part| part.string_value().is_some()) {
        return;
    }
    let profiles = PROFILES.get_or_init(|| {
        let catalog: StringCatalog = serde_json::from_str(include_str!(
            "../../emitter/literal_order/native-5.9.3.json"
        ))
        .expect("versioned native literal allocations");
        assert_eq!(catalog.schema, 1);
        assert_eq!(catalog.typescript, "5.9.3");
        catalog
            .profiles
            .into_iter()
            .map(|profile| {
                let ranks = profile
                    .values
                    .into_iter()
                    .enumerate()
                    .filter_map(|(rank, value)| {
                        value
                            .as_str()
                            .map(|text| (text.encode_utf16().collect(), rank))
                    })
                    .collect();
                (format!("{}:{}", profile.target, profile.mode), ranks)
            })
            .collect()
    });
    let Some(ranks) = profiles.get(profile) else {
        return;
    };
    parts.sort_by_key(|part| {
        part.string_value()
            .and_then(|value| ranks.get(value.units()).copied())
            .unwrap_or(usize::MAX)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_library_allocation_order_precedes_mapping_and_keeps_fresh_order() {
        let literals = || {
            vec![
                Type::Literal("\"word\"".into()),
                Type::Literal("'other'".into()),
            ]
        };
        let mut es5 = literals();
        order_strings(&mut es5, "es5:explicit");
        assert_eq!(es5, literals());
        let mut es2022 = literals();
        order_strings(&mut es2022, "es2022:explicit");
        assert_eq!(es2022, literals().into_iter().rev().collect::<Vec<_>>());
        let mut fresh = vec![
            Type::Literal("\"z_fresh_intrinsic\"".into()),
            Type::Literal("\"a_fresh_intrinsic\"".into()),
        ];
        let before = fresh.clone();
        order_strings(&mut fresh, "es2022:explicit");
        assert_eq!(fresh, before);
    }
}
