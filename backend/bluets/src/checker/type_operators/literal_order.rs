// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Pinned default-library cache order affects literal-union presentation.

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
