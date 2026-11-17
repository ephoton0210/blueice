// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static checking preserves semantic errors and frozen subset admission.

use blueice_bluets::{
    compile, CompilerOptions, DiagnosticCode, EcmaTarget, MapLoader, ModuleSource,
};

fn check(source: &str, decorators: bool) -> blueice_bluets::Compilation {
    let id = "program:///main.ts";
    compile(
        id,
        &MapLoader::from([ModuleSource::new(id, source)]),
        CompilerOptions {
            target: EcmaTarget::Es2022,
            no_emit: true,
            experimental_decorators: decorators,
            ..CompilerOptions::default()
        },
    )
}

#[test]
fn a_static_legacy_accessor_pair_retains_its_native_semantic_error() {
    let result = check(
        include_str!("fixtures/typescript_oracle/legacy-error-getter-and-setter/main.ts"),
        true,
    );
    assert!(result.has_errors());
    assert!(result.output.is_none());
    assert!(result.debug_info.is_none());
    assert!(result.diagnostics.iter().any(|diagnostic| diagnostic
        .typescript
        .as_ref()
        .is_some_and(|typescript| typescript.code == 1207)));
}

#[test]
fn static_parameter_properties_retain_the_existing_constructor_subset_boundary() {
    let result = check(
        include_str!(
            "fixtures/typescript_oracle/class-param-property-base-super-in-branch/main.ts"
        ),
        false,
    );
    assert!(result.has_errors());
    assert!(result.output.is_none());
    assert!(result.debug_info.is_none());
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::UnsupportedSyntax
            && diagnostic.message.contains("top-level `super(...)`")
    }));
}

#[test]
fn a_supported_static_parameter_property_keeps_metadata_without_emitting() {
    let result = check(
        include_str!("fixtures/typescript_oracle/class-param-property-derived/main.ts"),
        false,
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(result.output.is_none());
    assert!(result.debug_info.is_some());
}
