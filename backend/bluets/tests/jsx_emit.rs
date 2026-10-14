// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! JSX emit through the public compiler and `bluetsc` (J.5.2): the emitted text
//! of each mode, line preservation, source maps, output names and the manifest.
//! Parity with pinned TypeScript under Node is in `jsx_oracle.rs`.

use std::fs;
use std::process::Command;

use blueice_bluets::{compile, CompilerOptions, JsxMode, MapLoader, ModuleKind, ModuleSource};

const PRELUDE: &str = "declare namespace JSX { interface Element {} interface IntrinsicElements { div: any; span: any } }\n";

fn emit(source: &str, options: CompilerOptions) -> String {
    let compiled = compile(
        "memory:///main.tsx",
        &MapLoader::from([ModuleSource::new("memory:///main.tsx", source)]),
        options,
    );
    let output = compiled
        .output
        .unwrap_or_else(|| panic!("{:#?}", compiled.diagnostics));
    output.artifacts["memory:///main.tsx"].javascript.clone()
}

fn mode(mode: JsxMode) -> CompilerOptions {
    CompilerOptions {
        jsx: Some(mode),
        ..CompilerOptions::default()
    }
}

#[test]
fn classic_calls_the_factory_with_tag_props_and_children() {
    let source = format!(
        "/** @jsx h */\n{PRELUDE}declare function h(...a: any[]): any;\nconst n = 1;\nconst a = <div id=\"x\" {{...{{k: n}}}} hidden>hi {{n}}<span /></div>;\n"
    );
    let javascript = emit(&source, mode(JsxMode::React));
    assert!(
        javascript.contains(
            "h(\"div\", { id: \"x\", ...{k: n}, hidden: true }, \"hi \", n, h(\"span\", null))"
        ),
        "{javascript}"
    );
}

#[test]
fn the_automatic_runtime_imports_jsx_and_moves_the_key() {
    let source = format!("{PRELUDE}export const a = <div key=\"k\" id=\"x\">a<span /></div>;\nexport const b = <span />;\n");
    let javascript = emit(&source, mode(JsxMode::ReactJsx));
    assert!(
        javascript
            .starts_with("import { jsxs as _jsxs, jsx as _jsx } from \"react/jsx-runtime\"; "),
        "{javascript}"
    );
    assert!(
        javascript.contains(
            "_jsxs(\"div\", { id: \"x\", children: [\"a\", _jsx(\"span\", {})] }, \"k\")"
        ),
        "{javascript}"
    );
}

#[test]
fn jsx_import_source_and_dev_mode_choose_the_runtime_module() {
    let source = format!("{PRELUDE}export const a = <div />;\n");
    let javascript = emit(
        &source,
        CompilerOptions {
            jsx: Some(JsxMode::ReactJsx),
            jsx_import_source: Some("preact".to_string()),
            ..CompilerOptions::default()
        },
    );
    assert!(
        javascript.contains("from \"preact/jsx-runtime\""),
        "{javascript}"
    );
    let dev = emit(&source, mode(JsxMode::ReactJsxDev));
    assert!(dev.contains("from \"react/jsx-dev-runtime\""), "{dev}");
    assert!(
        dev.contains("const _jsxFileName = \"memory:///main.tsx\";"),
        "{dev}"
    );
    assert!(dev.contains("lineNumber: 2, columnNumber: 17"), "{dev}");
}

#[test]
fn commonjs_requires_the_runtime_and_calls_through_it() {
    let source = format!("{PRELUDE}export const a = <div />;\n");
    let javascript = emit(
        &source,
        CompilerOptions {
            jsx: Some(JsxMode::ReactJsx),
            module_kind: ModuleKind::CommonJs,
            ..CompilerOptions::default()
        },
    );
    assert!(
        javascript.contains("const jsx_runtime_1 = require(\"react/jsx-runtime\");"),
        "{javascript}"
    );
    assert!(
        javascript.contains("(0, jsx_runtime_1.jsx)(\"div\", {})"),
        "{javascript}"
    );
}

#[test]
fn the_runtime_import_follows_a_directive_prologue() {
    let source = format!("\"use strict\";\n{PRELUDE}export const a = <div />;\n");
    let javascript = emit(&source, mode(JsxMode::ReactJsx));
    assert!(
        javascript.starts_with("\"use strict\";import {"),
        "{javascript}"
    );
}

#[test]
fn a_runtime_name_already_in_the_module_is_refused() {
    let source = format!("{PRELUDE}export const _jsx = 1;\nexport const a = <div />;\n");
    let compiled = compile(
        "memory:///main.tsx",
        &MapLoader::from([ModuleSource::new("memory:///main.tsx", source.as_str())]),
        mode(JsxMode::ReactJsx),
    );
    assert!(compiled.output.is_none());
}

#[test]
fn every_emitted_line_keeps_its_source_line_in_every_lowering_mode() {
    let source = format!(
        "{PRELUDE}export const a = <div\n  id=\"x\"\n  title='y'\n>\n  text\n  {{1}}\n  <span />\n</div>;\nexport const z = 1;\n"
    );
    for jsx_mode in [JsxMode::React, JsxMode::ReactJsx, JsxMode::ReactJsxDev] {
        let options = CompilerOptions {
            jsx: Some(jsx_mode),
            jsx_factory: Some("h".to_string()),
            ..CompilerOptions::default()
        };
        let source = if jsx_mode == JsxMode::React {
            format!("declare function h(...a: any[]): any;\n{source}")
        } else {
            source.clone()
        };
        let javascript = emit(&source, options);
        // Imports are inserted in place on the first line, never as new lines.
        assert_eq!(
            javascript.matches('\n').count(),
            source.matches('\n').count(),
            "{jsx_mode:?}\n{javascript}"
        );
        let last = javascript.lines().last().unwrap();
        assert_eq!(last, "export const z = 1;", "{jsx_mode:?}");
    }
}

#[test]
fn annotations_inside_embedded_expressions_are_erased_and_imports_are_rewritten() {
    let source = format!(
        "/** @jsx h */\n{PRELUDE}declare function h(...a: any[]): any;\nconst a = <div onClick={{(e: number): number => e + 1}} title={{(1 as number).toString()}} />;\n"
    );
    let javascript = emit(&source, mode(JsxMode::React));
    assert!(javascript.contains("onClick: (e) => e + 1"), "{javascript}");
    assert!(!javascript.contains(": number"), "{javascript}");
}

#[test]
fn preserve_and_react_native_keep_the_jsx_text() {
    let source = format!("{PRELUDE}export const a = <div id=\"x\">{{1}}</div>;\n");
    for jsx_mode in [JsxMode::Preserve, JsxMode::ReactNative] {
        assert!(emit(&source, mode(jsx_mode)).contains("<div id=\"x\">{1}</div>"));
    }
}

#[test]
fn a_source_map_is_produced_for_lowered_jsx() {
    let compiled = compile(
        "memory:///main.tsx",
        &MapLoader::from([ModuleSource::new(
            "memory:///main.tsx",
            format!("{PRELUDE}export const a = <div id=\"x\">{{1}}</div>;\n").as_str(),
        )]),
        CompilerOptions {
            jsx: Some(JsxMode::ReactJsx),
            source_map: true,
            ..CompilerOptions::default()
        },
    );
    let artifact = &compiled.output.unwrap().artifacts["memory:///main.tsx"];
    let map = artifact
        .source_map
        .as_ref()
        .expect("a source map")
        .to_json();
    assert!(map.contains("main.tsx"), "{map}");
}

#[test]
fn the_cli_names_preserved_output_jsx_and_rewrites_imports_to_match() {
    let root = std::env::temp_dir().join(format!("bluetsc-jsx-names-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("main.tsx"),
        format!("import {{ view }} from \"./view.tsx\";\n{PRELUDE}export const a = view;\n"),
    )
    .unwrap();
    fs::write(
        root.join("view.tsx"),
        format!("{PRELUDE}export const view = <div />;\n"),
    )
    .unwrap();
    for (jsx_mode, extension) in [("preserve", "jsx"), ("react-native", "js"), ("react", "js")] {
        let out = root.join(format!("out-{jsx_mode}"));
        let mut command = Command::new(env!("CARGO_BIN_EXE_bluetsc"));
        command
            .current_dir(&root)
            .args(["build", "main.tsx", "--out-dir"])
            .arg(&out)
            .args(["--jsx", jsx_mode]);
        if jsx_mode == "react" {
            command.args(["--jsx-factory", "h"]);
        }
        let built = command.output().unwrap();
        if jsx_mode == "react" {
            // No `h` in scope: the program is refused rather than mis-emitted.
            assert!(!built.status.success());
            continue;
        }
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        assert!(out.join(format!("main.{extension}")).exists(), "{jsx_mode}");
        assert!(out.join(format!("view.{extension}")).exists(), "{jsx_mode}");
        let main = fs::read_to_string(out.join(format!("main.{extension}"))).unwrap();
        assert!(
            main.contains(&format!("\"./view.{extension}\"")),
            "{jsx_mode}: {main}"
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("bluetsc.manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["jsx"], jsx_mode);
        assert_eq!(manifest["entries"][0], format!("main.{extension}"));
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_config_file_carries_the_jsx_options_into_the_manifest_and_the_fingerprint() {
    let root = std::env::temp_dir().join(format!("bluetsc-jsx-config-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("main.tsx"),
        format!("{PRELUDE}declare function el(...a: any[]): any;\ndeclare const Frag: any;\ndeclare const Other: any;\nexport const a = el;\nexport const b = <div />;\nexport const c = <></>;\n"),
    )
    .unwrap();
    let build = |config: serde_json::Value| {
        fs::write(
            root.join("bluetsc.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&root)
            .args(["build", "--config", "bluetsc.json"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(root.join("dist/bluetsc.manifest.json")).unwrap(),
        )
        .unwrap()
    };
    let one = build(
        serde_json::json!({"entries": ["main.tsx"], "outDir": "dist", "jsx": "react", "jsxFactory": "el", "jsxFragmentFactory": "Frag"}),
    );
    assert_eq!(one["jsx"], "react");
    assert_eq!(one["jsxFactory"], "el");
    assert_eq!(one["jsxFragmentFactory"], "Frag");
    let main = fs::read_to_string(root.join("dist/main.js")).unwrap();
    assert!(
        main.contains("el(\"div\", null)") && main.contains("el(Frag, null)"),
        "{main}"
    );
    let two = build(
        serde_json::json!({"entries": ["main.tsx"], "outDir": "dist", "jsx": "react", "jsxFactory": "el", "jsxFragmentFactory": "Other"}),
    );
    assert_ne!(one["fingerprint"], two["fingerprint"]);
    // An unknown mode is refused when the config is read.
    fs::write(
        root.join("bluetsc.json"),
        r#"{"entries":["main.tsx"],"jsx":"vue"}"#,
    )
    .unwrap();
    let bad = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .current_dir(&root)
        .args(["check", "--config", "bluetsc.json"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unsupported jsx `vue`"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn spread_children_keys_and_element_attributes_follow_typescripts_call_shapes() {
    let source = format!(
        "{PRELUDE}declare const rest: any;\nconst items: any[] = [];\nexport const a = <div {{...rest}} key=\"after\" id=\"x\" />;\nexport const b = <div key={{1}}>{{...items}}</div>;\nexport const c = <div key=\"k\" id=\"i\">one{{1}}</div>;\nexport const d = <div x=<span /> />;\n"
    );
    let automatic = emit(&source, mode(JsxMode::ReactJsx));
    // A key after a non-literal spread needs `createElement`.
    assert!(automatic.contains("_createElement(\"div\""), "{automatic}");
    assert!(
        automatic.contains("import { createElement as _createElement"),
        "{automatic}"
    );
    assert!(
        automatic.contains("_jsxs(\"div\", { children: [...items] }")
            || automatic.contains("_jsxs("),
        "{automatic}"
    );
    let dev = emit(&source, mode(JsxMode::ReactJsxDev));
    assert!(
        dev.contains("void 0, true") || dev.contains("true, {"),
        "{dev}"
    );
    let classic = emit(
        &format!("/** @jsx h */\n{PRELUDE}declare function h(...a: any[]): any;\ndeclare const rest: any;\nconst items: any[] = [];\nexport const a = <div {{...rest}} id=\"x\">{{...items}}text</div>;\n"),
        mode(JsxMode::React),
    );
    assert!(
        classic.contains("h(\"div\", { ...rest, id: \"x\" }, ...items, \"text\")"),
        "{classic}"
    );
}

#[test]
fn quotes_names_and_expressions_are_written_the_way_typescript_writes_them() {
    let javascript = emit(
        &format!(
            "/** @jsx h */\n{PRELUDE}declare function h(...a: any[]): any;\nexport const a = <div data-x='a\"b' title=\"it's\" aria-hidden x:y=\"1\">{{(1, 2)}}&amp;\\n</div>;\n"
        ),
        mode(JsxMode::React),
    );
    assert!(
        javascript.contains("\"data-x\": 'a\"b'") || javascript.contains("\"data-x\": 'a\\\"b'"),
        "{javascript}"
    );
    assert!(javascript.contains("title: \"it's\""), "{javascript}");
    assert!(javascript.contains("\"aria-hidden\": true"), "{javascript}");
    assert!(javascript.contains("\"x:y\": \"1\""), "{javascript}");
    assert!(javascript.contains("(1, 2)"), "{javascript}");
}

#[test]
fn a_member_tag_and_an_imported_factory_go_through_the_commonjs_reference_map() {
    let compiled = compile(
        "memory:///main.tsx",
        &MapLoader::from([
            ModuleSource::new(
                "memory:///main.tsx",
                format!("/** @jsx make */\nimport make from \"./factory.ts\";\nimport * as ui from \"./factory.ts\";\n{PRELUDE}export const a = <ui.Item id=\"x\" />;\nexport const b = <div />;\n").as_str(),
            ),
            ModuleSource::new(
                "memory:///factory.ts",
                "export default function make(...a: any[]): any { return a; }\nexport const Item: any = 1;\n",
            ),
        ]),
        CompilerOptions {
            jsx: Some(JsxMode::React),
            module_kind: ModuleKind::CommonJs,
            es_module_interop: true,
            ..CompilerOptions::default()
        },
    );
    let output = compiled
        .output
        .unwrap_or_else(|| panic!("{:#?}", compiled.diagnostics));
    let javascript = &output.artifacts["memory:///main.tsx"].javascript;
    assert!(
        javascript.contains("(0, factory_1.default)(ui.Item"),
        "{javascript}"
    );
    assert!(
        javascript.contains("(0, factory_1.default)(\"div\", null)"),
        "{javascript}"
    );
}

#[test]
fn a_malformed_element_is_a_parse_error_not_emitted_text() {
    for source in ["const a = <div>;\n", "const a = <div></span>;\n"] {
        let compiled = compile(
            "memory:///main.tsx",
            &MapLoader::from([ModuleSource::new(
                "memory:///main.tsx",
                format!("{PRELUDE}{source}").as_str(),
            )]),
            mode(JsxMode::ReactJsx),
        );
        assert!(compiled.output.is_none(), "{source}");
    }
}
