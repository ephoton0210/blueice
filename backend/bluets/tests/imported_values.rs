// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Checked module surfaces through the public compiler boundary (K.1.3).

use blueice_bluets::{
    compile, CompilerOptions, DiagnosticCode, IncrementalCompiler, MapLoader, ModuleSource,
    RuntimePolicy, SymbolKind, Type,
};

fn loader(main: &str, dependency: &str) -> MapLoader {
    MapLoader::from([
        ModuleSource::new("memory:///main.ts", main),
        ModuleSource::new("memory:///dep.ts", dependency),
    ])
}

#[test]
fn checked_imports_and_inferred_variables_keep_their_debugger_types() {
    let compilation = compile(
        "memory:///main.ts",
        &loader(
            "import {value} from './dep.ts'; export const result = value;",
            "export let value = 3;",
        ),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors(), "{:#?}", compilation.diagnostics);
    let checked = compilation.checked.as_ref().unwrap();
    for (module, name, kind) in [
        ("memory:///main.ts", "value", SymbolKind::Import),
        ("memory:///main.ts", "result", SymbolKind::Variable),
        ("memory:///dep.ts", "value", SymbolKind::Variable),
    ] {
        let symbol = checked.modules[module]
            .symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.kind == kind)
            .unwrap();
        assert_eq!(symbol.value_type, Some(Type::Number), "{symbol:?}");
    }
    assert_eq!(
        compilation.output.unwrap().artifacts["memory:///main.ts"]
            .declaration
            .as_deref(),
        Some("export declare const result: number;\n")
    );
}

#[test]
fn exported_private_types_stay_bound_to_the_exporter() {
    for target in ["number", "string"] {
        let compilation = compile(
            "memory:///main.ts",
            &loader(
                &format!("import {{value}} from './dep.ts'; interface Shape {{count: string;}} const result: {target} = value.count;"),
                "interface Shape {count: number;} export const value: Shape = {count: 3};",
            ),
            CompilerOptions::default(),
        );
        assert_eq!(compilation.has_errors(), target == "string");
        assert_eq!(compilation.output.is_some(), target == "number");
        if target == "string" {
            assert!(compilation
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == DiagnosticCode::TypeMismatch));
        }
    }
}

#[test]
fn incremental_checks_retain_and_replace_dependency_value_surfaces() {
    let mut compiler = IncrementalCompiler::new();
    let options = CompilerOptions::default();
    let first = compiler.compile(
        "memory:///main.ts",
        &loader(
            "import {value} from './dep.ts'; const result: number = value;",
            "export let value = 3;",
        ),
        options.clone(),
    );
    assert!(!first.compilation.has_errors());
    let second = compiler.compile(
        "memory:///main.ts",
        &loader(
            "import {value} from './dep.ts'; const result: string = value;",
            "export let value = 3;",
        ),
        options.clone(),
    );
    assert!(second.reused_checked_modules.contains("memory:///dep.ts"));
    assert!(second.compilation.has_errors());
    assert!(second.compilation.output.is_none());
    let third = compiler.compile(
        "memory:///main.ts",
        &loader(
            "import {value} from './dep.ts'; const result: number = value;",
            "export let value = 'wrong';",
        ),
        options,
    );
    assert!(third.rechecked_modules.contains("memory:///main.ts"));
    assert!(third.rechecked_modules.contains("memory:///dep.ts"));
    assert!(third.compilation.has_errors());
}

#[test]
fn inferred_imports_do_not_turn_transpile_only_into_checked_output() {
    let compilation = compile(
        "memory:///main.ts",
        &loader(
            "import {value} from './dep.ts'; const result: string = value;",
            "export const value: number = 3;",
        ),
        CompilerOptions {
            runtime_policy: RuntimePolicy::TranspileOnly,
            ..CompilerOptions::default()
        },
    );
    assert!(!compilation.has_errors());
    assert!(compilation.output.is_some());
}
