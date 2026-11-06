// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace helpers preserve the pinned compiler's own-property behavior.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn private_namespace_interop_preserves_non_enumerable_own_properties() {
    let source = "import * as dependency from './dep.js'; export const imported:any=dependency;";
    let entry = "memory:///main.ts";
    let output = compile(
        entry,
        &MapLoader::from([
            ModuleSource::new(entry, source),
            ModuleSource::new("memory:///dep.js", "export const value=3;"),
        ]),
        CompilerOptions {
            target: EcmaTarget::Es2022,
            module_kind: ModuleKind::CommonJs,
            es_module_interop: true,
            source_map: true,
            ..CompilerOptions::default()
        },
    );
    assert!(!output.has_errors(), "{:?}", output.diagnostics);
    let artifact = &output.output.unwrap().artifacts[entry];
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_interop_namespace.cjs",
    );
    let mut child = Command::new("node")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            &serde_json::to_vec(&serde_json::json!({
                "source": source,
                "javascript": artifact.javascript,
            }))
            .unwrap(),
        )
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}
