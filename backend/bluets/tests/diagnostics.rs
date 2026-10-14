// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Diagnostic compatibility through the public compiler boundary.

use blueice_bluets::{
    compile, CompilerOptions, Diagnostic, DiagnosticCode, MapLoader, ModuleLoader, ModuleSource,
    SourceSpan, DIAGNOSTICS_VERSION,
};

#[test]
fn checked_errors_preserve_bts_identity_and_expose_a_typescript_counterpart() {
    let source = "const value: number = 'wrong';";
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert!(result.output.is_none());
    let diagnostic = &result.diagnostics[0];
    assert_eq!(diagnostic.code, DiagnosticCode::TypeMismatch);
    assert_eq!(diagnostic.code.to_string(), "BTS3003");
    let json = diagnostic.to_json();
    assert_eq!(json["btsCode"], "BTS3003");
    assert_eq!(json["rawMessage"], diagnostic.message);
    assert_eq!(json["typescript"]["code"], 2322);
    assert_eq!(
        json["typescript"]["messageTemplate"],
        "Type '{0}' is not assignable to type '{1}'."
    );
    assert!(json["noTypeScriptCounterpart"].is_null());
    assert!(json.get("source").is_none());
    assert_eq!(DIAGNOSTICS_VERSION, "typescript-5.9.3-diagnostics-v3");
}

#[test]
fn an_owner_resource_budget_retains_a_precise_blue_only_diagnostic() {
    let mut options = CompilerOptions::default();
    options.limits.max_modules = 0;
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", "const value = 1;")]),
        options,
    );
    assert!(result.has_errors());
    assert!(result.output.is_none());
    let diagnostic = &result.diagnostics[0];
    assert_eq!(diagnostic.code, DiagnosticCode::ResourceLimit);
    let json = diagnostic.to_json();
    assert_eq!(json["btsCode"], "BTS9000");
    assert!(json["typescript"].is_null());
    assert!(json["noTypeScriptCounterpart"]
        .as_str()
        .unwrap()
        .contains("resource budget"));
    assert!(diagnostic.message.contains("module limit"));
}

#[test]
fn an_owner_loader_refusal_is_explicit_even_with_a_custom_message() {
    struct DeniedLoader;
    impl ModuleLoader for DeniedLoader {
        fn load(&self, _: &str) -> Result<ModuleSource, String> {
            Err("The owner denied this input request.".into())
        }
    }
    let result = compile(
        "memory:///main.ts",
        &DeniedLoader,
        CompilerOptions::default(),
    );
    assert!(result.has_errors());
    assert!(result.output.is_none());
    let diagnostic = &result.diagnostics[0];
    assert_eq!(diagnostic.code, DiagnosticCode::ModuleNotFound);
    let json = diagnostic.to_json();
    assert!(json["typescript"].is_null());
    assert!(json["noTypeScriptCounterpart"]
        .as_str()
        .unwrap()
        .contains("owner-controlled"));
}

#[test]
fn a_missing_module_counterpart_does_not_depend_on_os_error_wording() {
    for detail in [
        "No such file or directory (os error 2)",
        "The system cannot find the path specified. (os error 3)",
    ] {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::ModuleNotFound,
            SourceSpan::new("memory:///main.ts", 0, 1),
            format!("cannot resolve `absent` from `main.ts`: {detail}"),
        );
        assert_eq!(diagnostic.typescript.as_ref().unwrap().code, 2792);
        assert_eq!(diagnostic.to_json()["btsCode"], "BTS2000");
    }
}

#[test]
fn a_valid_computed_member_emits_without_a_subset_diagnostic() {
    let source = "class Box { ['value'](): number { return 1; } }";
    let result = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
        CompilerOptions::default(),
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(result.output.is_some());
}

#[test]
fn a_compatible_function_field_refusal_does_not_invent_an_override_error() {
    let source = "function work(): number { return 1; } class Parent { run(): number { return 1; } } class Child extends Parent { run: () => number = work; }";
    let result = compile(
        "memory:///override.ts",
        &MapLoader::from([ModuleSource::new("memory:///override.ts", source)]),
        CompilerOptions::default(),
    );
    let refusals = result
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic
                .message
                .contains("as a property but the base class defines it as a method")
        })
        .collect::<Vec<_>>();
    assert!(!refusals.is_empty(), "{:?}", result.diagnostics);
    for diagnostic in refusals {
        assert!(diagnostic.typescript.is_none(), "{diagnostic:?}");
        assert!(diagnostic.to_json()["noTypeScriptCounterpart"]
            .as_str()
            .is_some());
    }
}

#[test]
fn semantic_counterparts_follow_renamed_declarations_and_bindings() {
    for (source, expected) in [
        ("class Skater { constructor(speed: number) {} } new Skater();", 2554),
        ("class Skater { constructor(speed: number); constructor(speed: string); constructor(speed: any) {} } new Skater(true);", 2769),
        ("class Skater { get speed(): number {} }", 2378),
        ("class Skater { set speed(value: number) { return 1; } }", 2408),
        ("class Skater { #speed = 1; } const skater = new Skater(); skater.#speed;", 18013),
        ("interface Position { x: number; } Position;", 2693),
        ("namespace Track { export interface Position { x: number; } } const position: Track.Missing = { x: 1 };", 2694),
        ("namespace Track { export const speed: number = 1; } const value: Track.speed = 1;", 2749),
        ("class Skater { static speed = 1; } const skater = new Skater(); skater.speed;", 2576),
        ("class Skater { #speed = 1; } const skater: Skater = {};", 2741),
        ("class Skater { private speed = 1; } const skater: Skater = { speed: 1 };", 2322),
        ("const distance: number = distance;", 2448),
        ("const position = { absent };", 18004),
    ] {
        let result = compile(
            "memory:///renamed.ts",
            &MapLoader::from([ModuleSource::new("memory:///renamed.ts", source)]),
            CompilerOptions::default(),
        );
        assert!(result.has_errors(), "{source}");
        assert!(result.output.is_none(), "{source}");
        assert!(result.diagnostics.iter().any(|diagnostic| {
            diagnostic.typescript.as_ref().is_some_and(|diagnostic| diagnostic.code == expected)
        }), "expected TS{expected}: {source}: {:?}", result.diagnostics);
    }
}

#[test]
fn failed_parsing_keeps_the_original_authorized_source_without_reloading() {
    use std::cell::Cell;
    struct OnceLoader {
        calls: Cell<usize>,
        source: &'static str,
    }
    impl ModuleLoader for OnceLoader {
        fn load(&self, id: &str) -> Result<ModuleSource, String> {
            self.calls.set(self.calls.get() + 1);
            assert_eq!(self.calls.get(), 1, "diagnostics must not reload source");
            Ok(ModuleSource::new(id, self.source))
        }
    }
    let source = "// 🧊\r\nclass Skater { get speed(value: number): number {} }";
    let loader = OnceLoader {
        calls: Cell::new(0),
        source,
    };
    let result = compile("memory:///invalid.ts", &loader, CompilerOptions::default());
    assert!(result.has_errors());
    assert!(result.project.modules.is_empty());
    assert_eq!(result.project.source("memory:///invalid.ts"), Some(source));
    assert_eq!(result.project.source("memory:///unrequested.ts"), None);
    let diagnostic = result.diagnostics[0].to_json();
    assert_eq!(diagnostic["typescript"]["code"], 1054);
    assert_eq!(
        diagnostic["typescript"]["position"],
        serde_json::json!({"line": 2, "column": 20, "length": 5})
    );
    assert!(!diagnostic.to_string().contains("🧊"));
}

#[test]
fn failed_source_bytes_participate_in_project_fingerprints() {
    let compile_source = |source| {
        compile(
            "memory:///invalid.ts",
            &MapLoader::from([ModuleSource::new("memory:///invalid.ts", source)]),
            CompilerOptions::default(),
        )
    };
    let first = compile_source("class Skater { get speed(value: number): number {} }");
    let changed = compile_source("class Skater { get speed(value: number): number {} }\r\n");
    assert!(first.has_errors() && changed.has_errors());
    assert_ne!(first.project_fingerprint, changed.project_fingerprint);
}

#[test]
fn explicit_source_coordinates_reject_non_boundary_or_out_of_range_spans() {
    for (start, end) in [(1, 2), (0, 9), (4, 3)] {
        let diagnostic = Diagnostic::error(
            DiagnosticCode::UnknownName,
            SourceSpan::new("memory:///input.ts", start, end),
            "cannot find name `missing`",
        )
        .with_typescript(2304, vec!["missing".into()])
        .with_source_position("🧊");
        assert!(diagnostic.to_json()["typescript"]["position"].is_null());
    }
    assert!(blueice_bluets::TypeScriptDiagnostic::is_known_compiler_option("incremental"));
    assert!(!blueice_bluets::TypeScriptDiagnostic::is_known_compiler_option("unknownOption"));
}
