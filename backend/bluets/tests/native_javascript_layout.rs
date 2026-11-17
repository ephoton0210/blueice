// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Printing is an explicit owner choice and preserves JavaScript token semantics.

use blueice_bluets::{
    compile, Compilation, CompilerOptions, IncrementalCompiler, JavaScriptLayout, MapLoader,
    ModuleSource,
};
use std::process::Command;

const ENTRY: &str = "memory:///main.ts";

fn build(source: &str, options: CompilerOptions) -> Compilation {
    let result = compile(
        ENTRY,
        &MapLoader::from([ModuleSource::new(ENTRY, source)]),
        options,
    );
    assert!(!result.has_errors(), "{source}: {:?}", result.diagnostics);
    result
}

fn javascript(result: &Compilation) -> &str {
    &result.output.as_ref().unwrap().artifacts[ENTRY].javascript
}

fn native() -> CompilerOptions {
    CompilerOptions {
        javascript_layout: JavaScriptLayout::TypeScript,
        ..CompilerOptions::default()
    }
}

#[test]
fn source_maps_finish_at_eof_without_a_final_source_newline() {
    for (source, expected) in [
        ("const value: number = 1;", "const value = 1;\n"),
        ("const label: string = '🌟';", "const label = '🌟';\n"),
        (
            "function read(): number { return 1; }",
            "function read() { return 1; }\n",
        ),
        (
            "class Box { value: number = 1; }",
            "class Box {\n    value = 1;\n}\n",
        ),
    ] {
        let result = build(
            source,
            CompilerOptions {
                source_map: true,
                ..native()
            },
        );
        assert_eq!(javascript(&result), expected);
        assert!(!result.output.as_ref().unwrap().artifacts[ENTRY]
            .source_map
            .as_ref()
            .unwrap()
            .mappings
            .is_empty());
    }
}

#[test]
fn layout_and_strict_directives_have_separate_cache_identities() {
    let loader = MapLoader::from([ModuleSource::new(ENTRY, "const value=1;\n")]);
    let mut compiler = IncrementalCompiler::new();
    let preserved = compiler.compile(ENTRY, &loader, CompilerOptions::default());
    let printed = compiler.compile(ENTRY, &loader, native());
    assert!(!printed.cache_hit);
    assert_ne!(
        preserved.compilation.project_fingerprint,
        printed.compilation.project_fingerprint
    );
    assert_eq!(javascript(&printed.compilation), "const value = 1;\n");
    let warm = compiler.compile(ENTRY, &loader, native());
    assert!(warm.cache_hit);
    let strict = compiler.compile(
        ENTRY,
        &loader,
        CompilerOptions {
            always_strict: true,
            ..native()
        },
    );
    assert!(!strict.cache_hit);
    assert_ne!(
        printed.compilation.project_fingerprint,
        strict.compilation.project_fingerprint
    );
    assert_eq!(
        javascript(&strict.compilation),
        "\"use strict\";\nconst value = 1;\n"
    );
}

#[test]
fn opaque_literals_comments_and_automatic_semicolons_keep_runtime_behavior() {
    for (source, expression, expected) in [
        ("let value = 1;\nvalue\n++value;\n", "value", "2"),
        ("function read() { return\n1; }\n", "read()", "undefined"),
        (
            "function read() { return 1; }\nread()\nread()\n",
            "read()",
            "1",
        ),
        ("const value = 1 + +1;\n", "value", "2"),
        (
            "const value = `a  { b }\n c`; // leave  this\n",
            "JSON.stringify(value)",
            "\"a  { b }\\n c\"",
        ),
        ("const value = /a  b/.test('a  b');\n", "value", "true"),
    ] {
        let result = build(source, native());
        let text = javascript(&result);
        let script = format!("{text}\nprocess.stdout.write(String({expression}));");
        let output = Command::new("node").args(["-e", &script]).output().unwrap();
        assert!(
            output.status.success(),
            "{source} => {text}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected,
            "{text}"
        );
        if source.contains("// leave  this") {
            assert!(text.contains("// leave  this"));
        }
    }
}

#[test]
fn parenthesized_initializers_and_empty_module_markers_use_native_spacing() {
    let result = build("export {};\nconst value = (1 + 2);\n", native());
    assert_eq!(javascript(&result), "export {};\nconst value = (1 + 2);\n");
}

#[test]
fn an_existing_strict_directive_is_not_duplicated() {
    let result = build(
        "'use strict'; const value=1;\n",
        CompilerOptions {
            always_strict: true,
            ..native()
        },
    );
    assert_eq!(javascript(&result), "'use strict';\nconst value = 1;\n");
}

#[test]
fn empty_module_markers_do_not_claim_a_runtime_source_owner() {
    let result = build(
        "export {};\n",
        CompilerOptions {
            source_map: true,
            ..native()
        },
    );
    let artifact = &result.output.as_ref().unwrap().artifacts[ENTRY];
    assert_eq!(artifact.javascript, "export {};\n");
    assert!(artifact.source_map.as_ref().unwrap().mappings.is_empty());
}

#[test]
fn a_named_import_keeps_a_separator_before_from() {
    let result = compile(
        ENTRY,
        &MapLoader::from([
            ModuleSource::new(
                ENTRY,
                "import { value } from './dep.ts'; export const result = value;\n",
            ),
            ModuleSource::new("memory:///dep.ts", "export const value = 1;\n"),
        ]),
        native(),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert!(javascript(&result).contains("import { value } from './dep.js';"));
}

#[test]
fn prefix_signs_keep_native_spacing_without_merging_update_tokens() {
    for (source, expected) in [
        ("const value = -1;", "const value = -1;\n"),
        ("const value = +1;", "const value = +1;\n"),
        ("const value = 2 - -1;", "const value = 2 - -1;\n"),
        ("const value = 2 + +1;", "const value = 2 + +1;\n"),
        ("const value = - -1;", "const value = - -1;\n"),
        ("const value = -(1 + 2);", "const value = -(1 + 2);\n"),
        ("const value = 4 - 2;", "const value = 4 - 2;\n"),
        ("const values = [-1, +2];", "const values = [-1, +2];\n"),
    ] {
        let result = build(source, native());
        assert_eq!(javascript(&result), expected, "{source}");
    }
}
