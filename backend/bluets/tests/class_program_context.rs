// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_bluets::{
    compile, CompilerOptions, EcmaTarget, IncrementalCompiler, JavaScriptLayout, MapLoader,
    ModuleKind, ModuleLoader, ModuleSource,
};

struct OwnerLoader(MapLoader);
impl ModuleLoader for OwnerLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        self.0.load(module_id)
    }
    fn resolve(&self, from: &str, specifier: &str) -> Result<String, String> {
        if from == "memory:///main.ts" && specifier == "./base.js" {
            Ok("memory:///base.ts".into())
        } else {
            Err("outside the explicitly authorized test edge".into())
        }
    }
}

#[test]
fn imported_base_class_uses_the_program_inheritance_abi_and_retires_it_incrementally() {
    let entry = "memory:///main.ts";
    let base = "memory:///base.ts";
    let base_source = "export class Base { value: number = 40; }";
    let options = CompilerOptions {
        target: EcmaTarget::Es5,
        module_kind: ModuleKind::CommonJs,
        libraries: Some(vec![EcmaTarget::Es2022]),
        source_map: true,
        declaration: true,
        javascript_layout: JavaScriptLayout::TypeScript,
        ..CompilerOptions::default()
    };
    let mut incremental = IncrementalCompiler::new();
    for (main, grouped) in [
        ("import { Base } from './base.js'; export const result: number = new Base().value;", false),
        ("import { Base } from './base.js'; function choose(): typeof Base { return Base; } export class Derived extends (choose()) {} export const result: number = new Derived().value;", true),
        ("import { Base } from './base.js'; export const result: number = new Base().value;", false),
    ] {
        let loader = OwnerLoader(MapLoader::from([
            ModuleSource::new(entry, main),
            ModuleSource::new(base, base_source),
            ModuleSource::new("memory:///unused.ts", "class Unselected extends Missing {}"),
        ]));
        let result = incremental.compile(entry, &loader, options.clone()).compilation;
        let fresh = compile(entry, &loader, options.clone());
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(!fresh.has_errors(), "{:?}", fresh.diagnostics);
        assert!(!result.project.modules.contains_key("memory:///unused.ts"));
        let output = result.output.as_ref().unwrap();
        let fresh_output = fresh.output.as_ref().unwrap();
        assert_eq!(output.artifacts.len(), fresh_output.artifacts.len());
        for (id, actual) in &output.artifacts {
            let expected = &fresh_output.artifacts[id];
            assert_eq!(actual.javascript, expected.javascript);
            assert_eq!(actual.declaration, expected.declaration);
            assert_eq!(actual.source_map, expected.source_map);
            assert_eq!(actual.declaration_map, expected.declaration_map);
        }
        let javascript = &output.artifacts[base].javascript;
        assert_eq!(javascript.contains("function __blueice_target_inherit_class_"), grouped,
            "cross-module inheritance requires the complete class compatibility group; independent programs retain Native factories: {javascript}");
        assert_eq!(javascript.contains("function __blueice_target_call_base_"), grouped);
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#[test]
fn lowered_computed_construction_selects_the_complete_class_abi() {
    let entry = "memory:///main.ts";
    for (construction, grouped, expected) in [
        ("new Box(40, 2)", false, "[42,[\"construct\"]]"),
        (
            "new (choose())(...values())",
            true,
            "[42,[\"choose\",\"values\",\"construct\"]]",
        ),
        (
            "new (choose())(...[...values()])",
            true,
            "[42,[\"choose\",\"values\",\"construct\"]]",
        ),
    ] {
        let source = format!("const log:string[]=[]; class Box{{value:number;constructor(first:number,second:number){{log.push(\"construct\");this.value=first+second;}}}} function choose():typeof Box{{log.push(\"choose\");return Box;}} function values():[number,number]{{log.push(\"values\");return [40,2];}} export const result:string=JSON.stringify([{construction}.value,log]);");
        let result = compile(
            entry,
            &MapLoader::from([ModuleSource::new(entry, source)]),
            CompilerOptions {
                target: EcmaTarget::Es5,
                module_kind: ModuleKind::CommonJs,
                libraries: Some(vec![EcmaTarget::Es2022]),
                javascript_layout: JavaScriptLayout::TypeScript,
                source_map: true,
                downlevel_iteration: true,
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let code = &result.output.as_ref().unwrap().artifacts[entry].javascript;
        assert_eq!(
            code.contains("function __blueice_target_inherit_class_"),
            grouped,
            "lowered constructor transport requires the complete class ABI: {code}"
        );
        assert_eq!(
            code.contains("function __blueice_target_call_base_"),
            grouped
        );
        let script = format!("(function(exports){{{code}\nconsole.log(exports.result);}})({{}});");
        let output = std::process::Command::new("node")
            .args(["-e", &script])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    }
}
