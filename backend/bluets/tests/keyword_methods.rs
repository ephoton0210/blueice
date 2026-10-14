// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};
fn diagnostics(source: &str) -> Vec<blueice_bluets::Diagnostic> {
    compile(
        "memory:///keyword.ts",
        &MapLoader::from([ModuleSource::new("memory:///keyword.ts", source)]),
        CompilerOptions::default(),
    )
    .diagnostics
}

#[test]
fn keyword_named_methods_infer_their_declared_result_through_chained_receivers() {
    let result=diagnostics("interface Handle { return(value: number): string; throw(value: number): boolean; } interface Maker { make(): Handle; } function close(maker: Maker): string { return maker.make().return(7); } function reject(handle: Handle): boolean { return handle.throw(1); }");
    assert!(result.is_empty(), "{result:?}");
}
#[test]
fn keyword_named_methods_keep_argument_validation() {
    let result=diagnostics("interface Handle { return(value: number): string; } function close(handle: Handle): string { return handle.return('invalid'); }");
    assert!(
        !result.is_empty(),
        "a keyword method must check its parameter type"
    );
}
#[test]
fn keyword_named_methods_keep_return_type_validation() {
    let result=diagnostics("interface Handle { return(value: number): string; } function close(handle: Handle): number { return handle.return(7); }");
    assert!(
        result
            .iter()
            .any(|d| d.code == blueice_bluets::DiagnosticCode::ReturnTypeMismatch),
        "{result:?}"
    );
}
