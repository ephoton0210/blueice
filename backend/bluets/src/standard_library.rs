// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original, static ECMAScript declarations. No loader or runtime authority.

use crate::{parse_module, EcmaTarget, Module};
use ring::digest::{digest, SHA256};
use serde::Serialize;
use std::sync::OnceLock;

pub const VERSION: &str = "blue-ts-ecma-lib-v1";
const BASE: &str = include_str!("standard_library/ecma-base.v1.d.ts");
const ES2022: &str = include_str!("standard_library/ecma-es2022.v1.d.ts");

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub version: &'static str,
    pub target: &'static str,
    pub sources: Vec<&'static str>,
    pub source_fingerprint: String,
}

fn sources(target: EcmaTarget) -> Vec<(&'static str, &'static str)> {
    let mut sources = vec![("ecma-base.v1.d.ts", BASE)];
    if target == EcmaTarget::Es2022 {
        sources.push(("ecma-es2022.v1.d.ts", ES2022));
    }
    sources
}

/// The version and exact selected content used by artifact/cache provenance.
pub fn identity(target: EcmaTarget) -> Identity {
    let sources = sources(target);
    let mut bytes = format!("{VERSION}\0{}\0", target.as_str()).into_bytes();
    for (name, text) in &sources {
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
    }
    let hash = digest(&SHA256, &bytes);
    Identity {
        version: VERSION,
        target: target.as_str(),
        sources: sources.iter().map(|(name, _)| *name).collect(),
        source_fingerprint: format!(
            "sha256:{}",
            hash.as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        ),
    }
}

pub(crate) fn modules(target: EcmaTarget) -> &'static [Module] {
    static OLD: OnceLock<Vec<Module>> = OnceLock::new();
    static NEW: OnceLock<Vec<Module>> = OnceLock::new();
    let cache = match target {
        EcmaTarget::Es2020 => &OLD,
        EcmaTarget::Es2022 => &NEW,
    };
    cache.get_or_init(|| {
        sources(target)
            .into_iter()
            .map(|(name, text)| {
                parse_module(format!("<builtin:{name}>"), text).expect(
                    "the original standard library must parse under BlueTS's declaration subset",
                )
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_parse_and_the_target_selects_exact_static_content() {
        assert_eq!(modules(EcmaTarget::Es2020).len(), 1);
        assert_eq!(modules(EcmaTarget::Es2022).len(), 2);
        let old = identity(EcmaTarget::Es2020);
        let new = identity(EcmaTarget::Es2022);
        assert_eq!(old.version, VERSION);
        assert_ne!(old.source_fingerprint, new.source_fingerprint);
        assert_eq!(new.sources, ["ecma-base.v1.d.ts", "ecma-es2022.v1.d.ts"]);
    }
}
