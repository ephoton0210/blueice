// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The ES5 factory completes initialization before assigning its outer class binding.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::process::Command;

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and complete supplemental ES5 decorator products"]
fn native_cycle_products_preserve_all_class_initialization_phases() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_decorator_class_phase.cjs",
    );
    let result = Command::new("node").arg(script).output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}

#[test]
fn static_initialization_and_all_legacy_decorators_see_the_unassigned_outer_class() {
    let prefix = "const events: any[] = []; function observe(label: string): string { events.push([label, typeof C]); return label; } function member(target: any, name: string): void { events.push([name, typeof C]); } function replace(value: any): any { events.push(['replace', typeof C, value.field]); function Replacement() {} return Replacement; } ";
    let class = "export class C { static field: string = observe('field'); static [observe('key')](): number { return 3; } static { observe('block'); } @member method(): number { return 1; } @member static other(): number { return 2; } } events.push(['after', typeof C]); export const result: any[] = events;\n";
    for decorated in [false, true] {
        for define in [false, true] {
            let source = format!(
                "{prefix}{}{class}",
                if decorated { "@replace " } else { "" }
            );
            for source_map in [false, true] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([ModuleSource::new(id, &source)]),
                    CompilerOptions {
                        target: EcmaTarget::Es5,
                        module_kind: ModuleKind::CommonJs,
                        experimental_decorators: true,
                        use_define_for_class_fields: Some(define),
                        source_map,
                        libraries: Some(vec![EcmaTarget::Es2022]),
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let javascript = &result.output.unwrap().artifacts[id].javascript;
                let execution = Command::new("node")
                    .arg("-e")
                    .arg(format!(
                        "{javascript}\nprocess.stdout.write(JSON.stringify(exports.result));"
                    ))
                    .output()
                    .unwrap();
                assert!(
                    execution.status.success(),
                    "{}",
                    String::from_utf8_lossy(&execution.stderr)
                );
                let mut expected = vec![
                    serde_json::json!(["key", "undefined"]),
                    serde_json::json!(["field", "undefined"]),
                    serde_json::json!(["block", "undefined"]),
                    serde_json::json!(["method", "undefined"]),
                    serde_json::json!(["other", "undefined"]),
                ];
                if decorated {
                    expected.push(serde_json::json!(["replace", "undefined", "field"]));
                }
                expected.push(serde_json::json!(["after", "function"]));
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&execution.stdout).unwrap(),
                    serde_json::json!(expected),
                    "decorated={decorated} define={define} maps={source_map}"
                );
            }
        }
    }
}

#[test]
fn same_named_namespace_classes_keep_their_own_initialization_inventory_without_maps() {
    let source = "const events: any[] = []; function observe(label: string): number { events.push([label, typeof A.C, B ? typeof B.C : 'undefined']); return 1; } function member(target: any, name: string): void { events.push([name, typeof A.C, B ? typeof B.C : 'undefined']); } namespace A { export class C { static field: number = observe('A'); @member first(): number { return 1; } } } namespace B { export class C { static field: number = observe('B'); @member second(): number { return 2; } } } export const result: any[] = events;\n";
    for source_map in [false, true] {
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                target: EcmaTarget::Es5,
                module_kind: ModuleKind::CommonJs,
                experimental_decorators: true,
                source_map,
                libraries: Some(vec![EcmaTarget::Es2022]),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let javascript = &result.output.unwrap().artifacts[id].javascript;
        let execution = Command::new("node")
            .arg("-e")
            .arg(format!(
                "{javascript}\nprocess.stdout.write(JSON.stringify(exports.result));"
            ))
            .output()
            .unwrap();
        assert!(
            execution.status.success(),
            "{}",
            String::from_utf8_lossy(&execution.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&execution.stdout).unwrap(),
            serde_json::json!([
                ["A", "undefined", "undefined"],
                ["first", "undefined", "undefined"],
                ["B", "function", "undefined"],
                ["second", "function", "undefined"]
            ]),
            "maps={source_map}"
        );
    }
}

#[test]
fn es5_legacy_esm_publication_preserves_the_original_class_binding() {
    let body =
        "function decorate(value: any): any { return value; } @decorate export class Box {}\n";
    for prefix in ["", "/* G-E5 😀 */\r\n"] {
        let source = format!("{prefix}{body}");
        let id = "memory:///main.ts";
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, &source)]),
            CompilerOptions {
                target: EcmaTarget::Es5,
                module_kind: ModuleKind::Esm,
                experimental_decorators: true,
                source_map: true,
                inline_sources: true,
                libraries: Some(vec![EcmaTarget::Es2022]),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        let code = &artifact.javascript;
        let map = artifact.source_map.as_ref().unwrap();
        let publication = code.find("export { Box }").unwrap();
        let name = publication + "export { ".len();
        let original = source.find("class Box").unwrap() + "class ".len();
        let position = |text: &str, offset| {
            let before = &text[..offset];
            (
                before.bytes().filter(|byte| *byte == b'\n').count(),
                before.rsplit('\n').next().unwrap().encode_utf16().count(),
            )
        };
        for offset in publication..name + "Box };".len() {
            let (line, column) = position(code, offset);
            let owner = map
                .original_position_at(code, line, column, 100_000, 32_000_000)
                .unwrap()
                .map(|owner| (owner.line, owner.column_utf16));
            let expected = if offset < name {
                None
            } else {
                Some(position(
                    &source,
                    original + usize::from(offset >= name + 3) * 3,
                ))
            };
            assert_eq!(owner, expected, "publication UTF-16 offset {offset}");
        }
    }
}
