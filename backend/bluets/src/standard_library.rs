// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original, static ECMAScript declarations. No loader or runtime authority.

use crate::{parse_module, EcmaTarget, Module};
use ring::digest::{digest, SHA256};
use serde::Serialize;
use std::sync::OnceLock;
mod intrinsics;
mod manifest;
pub(crate) mod presentation;
mod profiles;
const ASYNC_ITERATION: (&str, &str) = (
    "ecma-async-iteration.v2.d.ts",
    include_str!("standard_library/ecma-async-iteration.v2.d.ts"),
);

pub const VERSION: &str = "blue-ts-ecma-lib-v3";
const BASE: &str = include_str!("standard_library/ecma-base.v1.d.ts");
const ES2022: &str = include_str!("standard_library/ecma-es2022.v1.d.ts");
const ADDITIONS: &str = include_str!("standard_library/ecma-additions.v2.d.ts");
const DECORATORS: (&str, &str) = (
    "decorator-contexts.v2.d.ts",
    include_str!("standard_library/decorator-contexts.v2.d.ts"),
);
const SYMBOL_METADATA: (&str, &str) = (
    "symbol-metadata.v1.d.ts",
    include_str!("standard_library/symbol-metadata.v1.d.ts"),
);

pub(crate) fn symbol_metadata_module() -> &'static Module {
    static MODULE: OnceLock<Module> = OnceLock::new();
    MODULE.get_or_init(|| {
        parse_module(
            format!("<builtin:{}>", SYMBOL_METADATA.0),
            SYMBOL_METADATA.1,
        )
        .expect("static Symbol.metadata component parses")
    })
}

/// The ordinary edition identity plus an explicitly selected static component.
pub fn identity_with_components(
    target: EcmaTarget,
    libraries: Option<&[EcmaTarget]>,
    symbol_metadata: bool,
) -> Identity {
    let mut identity = identity_with_libraries(target, libraries);
    if (symbol_metadata
        || libraries.map_or(target, |editions| {
            editions.iter().copied().max().unwrap_or(target)
        }) == EcmaTarget::EsNext)
        && !identity.sources.is_empty()
    {
        let bytes = format!(
            "{}\0{}\0{}",
            identity.source_fingerprint, SYMBOL_METADATA.0, SYMBOL_METADATA.1
        );
        identity.sources.push(SYMBOL_METADATA.0);
        identity.libraries.push("esnext.decorators");
        identity.source_fingerprint = format!(
            "sha256:{}",
            digest(&SHA256, bytes.as_bytes())
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
    }
    identity
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub version: &'static str,
    pub target: &'static str,
    pub libraries: Vec<&'static str>,
    pub sources: Vec<&'static str>,
    pub source_fingerprint: String,
}

fn sources(target: EcmaTarget) -> Vec<(&'static str, &'static str)> {
    let mut sources = if matches!(target, EcmaTarget::Es2020 | EcmaTarget::Es2022) {
        let mut sources = vec![("ecma-base.v1.d.ts", BASE)];
        if target >= EcmaTarget::Es2022 {
            sources.push(("ecma-es2022.v1.d.ts", ES2022));
        }
        sources.push(("ecma-additions.v2.d.ts", ADDITIONS));
        sources.push(ASYNC_ITERATION);
        sources
    } else {
        profiles::sources(target)
    };
    sources.push(DECORATORS);
    sources.extend(manifest::sources(target));
    sources
}

/// The version and exact selected content used by artifact/cache provenance.
pub fn identity(target: EcmaTarget) -> Identity {
    let sources = sources(target);
    let version = if matches!(target, EcmaTarget::Es2020 | EcmaTarget::Es2022) {
        VERSION
    } else {
        profiles::VERSION
    };
    let mut bytes = format!("{version}\0{}\0", target.as_str()).into_bytes();
    for (name, text) in &sources {
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);
    }
    let hash = digest(&SHA256, &bytes);
    Identity {
        version,
        target: target.as_str(),
        libraries: vec![target.as_str()],
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

/// Identity of the explicitly selected library set, independently of emit syntax.
pub fn identity_with_libraries(target: EcmaTarget, libraries: Option<&[EcmaTarget]>) -> Identity {
    let Some(libraries) = libraries else {
        return identity(target);
    };
    let selected = libraries.iter().copied().max();
    let mut result = selected.map(identity).unwrap_or_else(|| Identity {
        version: profiles::VERSION,
        target: target.as_str(),
        libraries: Vec::new(),
        sources: Vec::new(),
        source_fingerprint: String::new(),
    });
    let bytes = format!(
        "{}\0{}\0{libraries:?}\0{}",
        result.version,
        target.as_str(),
        result.source_fingerprint
    );
    result.source_fingerprint = format!(
        "sha256:{}",
        digest(&SHA256, bytes.as_bytes())
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    result.target = target.as_str();
    result.libraries = libraries.iter().map(|edition| edition.as_str()).collect();
    result
}

pub(crate) fn modules(target: EcmaTarget) -> &'static [Module] {
    static CACHE: [OnceLock<Vec<Module>>; 11] = [const { OnceLock::new() }; 11];
    let cache = &CACHE[target as usize];
    cache.get_or_init(|| {
        sources(target)
            .into_iter()
            .map(|(name, text)| {
                let mut module = parse_module(format!("<builtin:{name}>"), text).expect(
                    "the original standard library must parse under BlueTS's declaration subset",
                );
                intrinsics::lower(&mut module);
                module
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_parse_and_the_target_selects_exact_static_content() {
        assert_eq!(modules(EcmaTarget::Es2020).len(), 12);
        assert_eq!(modules(EcmaTarget::Es2022).len(), 15);
        let old = identity(EcmaTarget::Es2020);
        let new = identity(EcmaTarget::Es2022);
        assert_eq!(old.version, VERSION);
        assert_ne!(old.source_fingerprint, new.source_fingerprint);
        assert_eq!(
            new.sources,
            [
                "ecma-base.v1.d.ts",
                "ecma-es2022.v1.d.ts",
                "ecma-additions.v2.d.ts",
                "ecma-async-iteration.v2.d.ts",
                "decorator-contexts.v2.d.ts",
                "ecma-core.v3.d.ts",
                "ecma-buffers.v3.d.ts",
                "ecma-iterable.v3.d.ts",
                "ecma-promise.v3.d.ts",
                "ecma-es2017.v3.d.ts",
                "ecma-es2018.v3.d.ts",
                "ecma-es2019.v3.d.ts",
                "ecma-es2020.v3.d.ts",
                "ecma-es2021.v3.d.ts",
                "ecma-es2022.v3.d.ts"
            ]
        );
    }
}
