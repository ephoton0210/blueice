// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static JSON inputs obey the owner's existing parser work bounds.

use blueice_bluets::{compile, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource};

const ENTRY: &str = "memory:///data.json";
const SOURCE: &str = "{\"label\":\"你好\",\"value\":42}";

fn options() -> CompilerOptions {
    CompilerOptions {
        resolve_json_module: true,
        ..CompilerOptions::default()
    }
}

fn refused(options: CompilerOptions, message: &str) {
    let result = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, SOURCE)]),
        options,
    );
    assert!(
        result.has_errors(),
        "JSON must retain the owner {message} limit"
    );
    assert!(result.output.is_none());
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::ResourceLimit && diagnostic.message.contains(message)
        }),
        "{:#?}",
        result.diagnostics
    );
}

#[test]
fn json_within_owner_limits_keeps_unicode_asset_bytes() {
    let result = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, SOURCE)]),
        options(),
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    assert_eq!(result.output.unwrap().assets[ENTRY], SOURCE);
}

#[test]
fn json_cannot_bypass_the_owner_source_byte_limit() {
    let mut options = options();
    options.limits.parser.max_source_bytes = SOURCE.len() - 1;
    refused(options, "byte");
}

#[test]
fn json_cannot_bypass_the_owner_token_limit() {
    let mut options = options();
    options.limits.parser.max_tokens = 2;
    refused(options, "token");
}

#[test]
fn json_cannot_bypass_the_owner_schema_depth_limit() {
    let mut options = options();
    options.limits.parser.max_type_depth = 1;
    refused(options, "depth");
}
