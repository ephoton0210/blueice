// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The four frozen safe home-object families retain complete public artifact contracts.
use std::process::Command;

#[test]
#[ignore = "requires pinned TypeScript/Acorn, an immutable CLI and its explicit SHA-256"]
fn safe_home_object_families_keep_complete_modules_and_logical_ends() {
    let compiler = std::env::var_os("BLUEICE_GE5_CLI_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_bluetsc").into());
    let hash = std::env::var_os("BLUEICE_GE5_EXPECTED_COMPILER_SHA256")
        .expect("explicit immutable CLI SHA-256");
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_safe_home_roles.cjs",
    );
    let output = Command::new("node")
        .arg("--experimental-vm-modules")
        .arg(script)
        .arg(compiler)
        .arg(hash)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn empty_private_callable_keeps_its_original_header_comment() {
    use blueice_bluets::{
        compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource,
    };
    let id = "memory:///main.ts";
    let source = "function dec(value:any):any{return value;}\nexport class C { @dec get #value()/* original header */:number {return 7;} run():number{return this.#value;} }\nexport const result=new C().run();\n";
    for target in [EcmaTarget::Es2015, EcmaTarget::Es2022, EcmaTarget::EsNext] {
        let result = compile(
            id,
            &MapLoader::from([ModuleSource::new(id, source)]),
            CompilerOptions {
                target,
                module_kind: ModuleKind::CommonJs,
                use_define_for_class_fields: Some(false),
                source_map: true,
                inline_sources: true,
                libraries: Some(vec![EcmaTarget::Es2022]),
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let artifact = &result.output.unwrap().artifacts[id];
        assert!(artifact.javascript.contains("/* original header */"));
        let execution = Command::new("node")
            .arg("-e")
            .arg(format!(
                "{}\nprocess.stdout.write(String(exports.result));",
                artifact.javascript
            ))
            .output()
            .unwrap();
        assert!(
            execution.status.success(),
            "{}",
            String::from_utf8_lossy(&execution.stderr)
        );
        assert_eq!(execution.stdout, b"7");
    }
}
