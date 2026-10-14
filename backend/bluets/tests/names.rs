// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Lexical diagnostics and host-policy boundaries through `compile` (K.1.1).

use blueice_bluets::{
    compile, CompilerOptions, DiagnosticCode, MapLoader, ModuleSource, RuntimePolicy,
};

fn checked(source: &str, options: CompilerOptions) -> blueice_bluets::Compilation {
    compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        options,
    )
}

#[test]
fn unknown_values_and_erased_types_point_to_the_identifier_and_prevent_output() {
    for (source, name, code) in [
        (
            "const result = 1 + missing;",
            "missing",
            DiagnosticCode::UnknownName,
        ),
        (
            "const result = `value:${missing}`;",
            "missing",
            DiagnosticCode::UnknownName,
        ),
        (
            "const result = 1 as Missing;",
            "Missing",
            DiagnosticCode::UnknownType,
        ),
        (
            "type Result = typeof missing;",
            "missing",
            DiagnosticCode::UnknownName,
        ),
        (
            "let result: number = result;",
            "result",
            DiagnosticCode::UsedBeforeDeclaration,
        ),
        (
            "namespace Area { export const first: number = later; } namespace Area { export const later: number = 1; }",
            "later",
            DiagnosticCode::UsedBeforeDeclaration,
        ),
    ] {
        let result = checked(source, CompilerOptions::default());
        assert!(result.output.is_none());
        let diagnostic = result.diagnostics.iter().find(|d| d.code == code).unwrap();
        assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], name);
    }
}

#[test]
fn parameter_defaults_see_outer_values_but_do_not_see_body_locals() {
    let valid = checked("const outer: number = 1; function f(value: number = outer): number { const outer: string = 'inner'; return value; }", CompilerOptions::default());
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
    let invalid = checked(
        "function f(value: number = local): number { var local: number = 1; return value; }",
        CompilerOptions::default(),
    );
    assert!(invalid
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::UnknownName));
}

#[test]
fn name_queries_keep_shadowed_bindings_distinct() {
    let valid = checked("const value: string = 'outer'; type Outer = typeof value; function f(value: number): typeof value { return value; } const text: Outer = 'ok'; const numberValue: number = f(1);", CompilerOptions::default());
    assert!(!valid.has_errors(), "{:#?}", valid.diagnostics);
    let invalid = checked(
        "const value: string = 'outer'; function f(value: number): typeof value { return 'bad'; }",
        CompilerOptions::default(),
    );
    assert!(invalid
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ReturnTypeMismatch));
}

#[test]
fn standard_name_recognition_does_not_replace_owner_call_declarations() {
    let source = "parseInt('1');";
    assert!(!checked(source, CompilerOptions::default()).has_errors());
    let options = CompilerOptions {
        require_declared_global_calls: true,
        ..CompilerOptions::default()
    };
    let restricted = checked(source, options.clone());
    assert!(restricted
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::UnknownName && d.message.contains("page profile")));
    let declared = checked(
        "declare function parseInt(value: string): number; parseInt('1');",
        options,
    );
    assert!(!declared.has_errors(), "{:#?}", declared.diagnostics);
    assert!(checked("document.textContent;", CompilerOptions::default()).has_errors());
}

#[test]
fn transpile_only_keeps_its_explicit_unchecked_policy() {
    let result = checked(
        "const result = missing;",
        CompilerOptions {
            runtime_policy: RuntimePolicy::TranspileOnly,
            ..CompilerOptions::default()
        },
    );
    assert!(!result.has_errors(), "{:#?}", result.diagnostics);
    assert!(result.output.is_some());
}

#[test]
fn contextual_keywords_used_as_values_need_bindings() {
    for name in ["any", "from", "of", "asserts", "is", "constructor"] {
        let source = format!("const result = {name};");
        let result = checked(&source, CompilerOptions::default());
        assert!(result.output.is_none(), "{name}");
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::UnknownName
                    && &source[diagnostic.span.start..diagnostic.span.end] == name
            }),
            "{name}: {:#?}",
            result.diagnostics
        );
    }
    for source in [
        "for (const key in of) {}",
        "for (let index = of; index < 1; index++) {}",
        "for (const item of [of]) {}",
    ] {
        let result = checked(source, CompilerOptions::default());
        assert!(result.output.is_none(), "{source}");
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == DiagnosticCode::UnknownName
                    && &source[diagnostic.span.start..diagnostic.span.end] == "of"
            }),
            "{source}: {:#?}",
            result.diagnostics
        );
    }
}
