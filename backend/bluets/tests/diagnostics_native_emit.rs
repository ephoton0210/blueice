// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Presentation never grants publication or relaxes compile/strict-runtime errors.
use blueice_bluets::{
    compile, CompilerOptions, MapLoader, ModuleLoader, ModuleSource, RuntimePolicy,
};

#[test]
fn native_error_emission_is_pure_fingerprinted_and_separate_from_compile_output() {
    let options = CompilerOptions::default();
    let loader = MapLoader::from([ModuleSource::new(
        "main.ts",
        "export const value: number = \"wrong\";\n",
    )]);
    let mut result = compile("main.ts", &loader, options.clone());
    assert!(result.has_errors());
    assert!(result.output.is_none());
    let output = result.emit_for_native_cli(&options).unwrap().unwrap();
    assert!(output.artifacts["main.ts"].javascript.contains("wrong"));
    assert!(result.output.is_none());
    let changed_options = CompilerOptions {
        source_map: true,
        ..options.clone()
    };
    assert!(result
        .emit_for_native_cli(&changed_options)
        .unwrap()
        .is_none());
    result
        .project
        .modules
        .get_mut("main.ts")
        .unwrap()
        .source
        .push_str("// changed\n");
    assert!(result.emit_for_native_cli(&options).unwrap().is_none());
}

#[test]
fn native_emission_refuses_parser_owner_budget_and_strict_runtime_failures() {
    for source in [
        "export const value: number = ;",
        "export const value: number = 1;\n",
    ] {
        let loader = MapLoader::from([ModuleSource::new("main.ts", source)]);
        let options = CompilerOptions {
            runtime_policy: RuntimePolicy::StrictRuntime,
            ..CompilerOptions::default()
        };
        let result = compile("main.ts", &loader, options.clone());
        assert!(result.emit_for_native_cli(&options).unwrap().is_none());
        assert!(result
            .emit_for_native_cli(&CompilerOptions::default())
            .unwrap()
            .is_none());
    }
    let loader = MapLoader::from([ModuleSource::new(
        "main.ts",
        "export const value: number = \"wrong\";",
    )]);
    let mut options = CompilerOptions::default();
    options.limits.parser.max_source_bytes = 1;
    let result = compile("main.ts", &loader, options.clone());
    assert!(result.emit_for_native_cli(&options).unwrap().is_none());
    let result = compile("missing.ts", &loader, CompilerOptions::default());
    assert!(result
        .emit_for_native_cli(&CompilerOptions::default())
        .unwrap()
        .is_none());
}

#[test]
fn nested_compiles_keep_performance_counters_isolated() {
    struct NestedLoader;
    impl ModuleLoader for NestedLoader {
        fn load(&self, id: &str) -> Result<ModuleSource, String> {
            let loader = MapLoader::from([ModuleSource::new(
                "inner.ts",
                "interface Shape { value: number; } export const value: Shape = { value: 1 };",
            )]);
            let inner = compile("inner.ts", &loader, CompilerOptions::default());
            assert!(!inner.has_errors());
            assert!(inner.performance.instantiations > 0);
            Ok(ModuleSource::new(id, "export const value: number = 1;\n"))
        }
    }
    let result = compile("main.ts", &NestedLoader, CompilerOptions::default());
    let performance = &result.performance;
    assert_eq!(performance.files, 1);
    assert_eq!(performance.lines, 2);
    assert_eq!(performance.retained_source_bytes, 32);
    assert_eq!(performance.instantiations, 0);
    assert_eq!(performance.symbols, 1);
    assert_eq!(performance.types, 1);
    assert!(performance.identifiers > 0);
    assert!(
        performance.total_time
            >= performance.load_time
                + performance.parse_time
                + performance.bind_time
                + performance.check_time
                + performance.emit_time
    );
}
