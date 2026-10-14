// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Script helper policies retain the native script/module distinction.

use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::process::Command;

#[test]
fn helper_imports_and_module_wrappers_leave_scripts_global() {
    let id = "memory:///main.ts";
    let loader = MapLoader::from([ModuleSource::new(
        id,
        "class Base {} class Derived extends Base {} var result: boolean = new Derived() instanceof Base;",
    )]);
    for kind in [ModuleKind::CommonJs, ModuleKind::Amd, ModuleKind::Umd] {
        for imports in [false, true] {
            for omit in [false, true] {
                let result = compile(
                    id,
                    &loader,
                    CompilerOptions {
                        target: EcmaTarget::Es5,
                        module_kind: kind,
                        import_helpers: imports,
                        no_emit_helpers: omit,
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                let output = result.output.unwrap();
                let javascript = &output.artifacts[id].javascript;
                assert!(!javascript.contains("tslib"), "{javascript}");
                assert!(!javascript.contains("define("), "{javascript}");
                if let Ok(version) = Command::new("node").arg("--version").output() {
                    assert!(version.status.success());
                    let observed = Command::new("node")
                        .args(["-e", "const vm=require('vm');const c={__extends(child,parent){Object.setPrototypeOf(child,parent);child.prototype=Object.create(parent.prototype);child.prototype.constructor=child;}};vm.runInNewContext(process.argv[1],c);if(c.result!==true)throw new Error('Script globals were hidden');"])
                        .arg(javascript)
                        .output()
                        .unwrap();
                    assert!(
                        observed.status.success(),
                        "{}",
                        String::from_utf8_lossy(&observed.stderr)
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn script_helper_observations_match_pinned_typescript() {
    let recorder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/module_script_helpers/record.cjs");
    let output = Command::new("node").arg(recorder).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
