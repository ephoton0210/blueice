// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Observed type-only library keys; these grant no member type or runtime value.

use crate::{EcmaTarget, SourceSpan, Type};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub(crate) const VERSION: &str = "bluets-type-operators/4/ts5.9.3";
#[derive(Deserialize)]
struct Catalog {
    schema: u8,
    typescript: String,
    profiles: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    target: String,
    mode: String,
    keysets: Vec<Keyset>,
    strings: Vec<String>,
}
#[derive(Deserialize)]
struct Keyset {
    name: String,
    keys: Vec<Key>,
    display: String,
}
#[derive(Deserialize)]
struct Key {
    kind: String,
    value: Option<String>,
}
struct Facts {
    keys: BTreeMap<String, (Type, String)>,
    strings: BTreeMap<String, usize>,
}
static PROFILES: OnceLock<BTreeMap<String, Facts>> = OnceLock::new();

fn profiles() -> &'static BTreeMap<String, Facts> {
    PROFILES.get_or_init(|| {
        let catalog: Catalog =
            serde_json::from_str(include_str!("type_operator_profiles/native-5.9.3.json"))
                .expect("versioned native key facts");
        assert_eq!(catalog.schema, 1);
        assert_eq!(catalog.typescript, "5.9.3");
        catalog
            .profiles
            .into_iter()
            .filter(|p| p.mode == "default")
            .map(|profile| {
                let keys = profile
                    .keysets
                    .into_iter()
                    .map(|set| {
                        let mut keys: Vec<_> = set
                            .keys
                            .into_iter()
                            .map(|key| match key.kind.as_str() {
                                "number" => Type::Number,
                                "text" => Type::Literal(
                                    serde_json::to_string(key.value.as_ref().expect("text key"))
                                        .unwrap(),
                                ),
                                "symbol" => Type::UniqueSymbol(SourceSpan::new(
                                    format!(
                                        "<typescript-key:{}/{}>",
                                        profile.target,
                                        key.value.as_deref().expect("symbol key")
                                    ),
                                    0,
                                    0,
                                )),
                                _ => panic!("invalid versioned native key"),
                            })
                            .collect();
                        let value = match keys.len() {
                            0 => Type::Never,
                            1 => keys.pop().unwrap(),
                            _ => Type::Union(keys),
                        };
                        (set.name, (value, set.display))
                    })
                    .collect();
                (
                    profile.target,
                    Facts {
                        keys,
                        strings: profile
                            .strings
                            .into_iter()
                            .enumerate()
                            .map(|(i, s)| (s, i))
                            .collect(),
                    },
                )
            })
            .collect()
    })
}

pub(crate) fn alias_name(owner: &str) -> String {
    format!("\0bluets:keyof:{owner}")
}

pub(crate) fn keys(target: EcmaTarget) -> impl Iterator<Item = (String, Type)> {
    profiles()
        .get(target.as_str())
        .expect("owned key profile")
        .keys
        .iter()
        .map(|(name, (value, _))| (name.clone(), value.clone()))
}

/// Use native diagnostic spelling only for the exact observed key union.
/// The type remains a real union during checking and contextual inference.
pub(crate) fn diagnostic(value: &Type) -> Option<&'static str> {
    let Type::Union(parts) = value else {
        return None;
    };
    if !parts.iter().any(|part| matches!(part,Type::UniqueSymbol(span) if span.module.starts_with("<typescript-key:"))) { return None; }
    profiles()
        .values()
        .flat_map(|p| p.keys.values())
        .find(|(keys, _)| keys == value)
        .map(|(_, display)| display.as_str())
}

pub(crate) fn order_strings(parts: &mut [Type], keyset: &Type) {
    if !parts
        .iter()
        .all(|part| matches!(part,Type::Literal(text) if text.starts_with(['\'', '"'])))
    {
        return;
    }
    let Some(profile) = profiles().values().find(|p| {
        p.keys
            .get("String")
            .is_some_and(|(value, _)| value == keyset)
    }) else {
        return;
    };
    parts.sort_by_key(|part| match part {
        Type::Literal(text) => crate::enum_eval::decode_plain_string(text)
            .and_then(|text| profile.strings.get(&text).copied())
            .unwrap_or(usize::MAX),
        _ => unreachable!(),
    });
}
