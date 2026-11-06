// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Runtime import elision preserves detached and preceding-node comment ownership.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn import_elision_preserves_native_attached_detached_and_trailing_comment_policy() {
    let mut rows = Vec::new();
    for prefix in [
        "/* attached */\n",
        "/* detached */\n\n",
        "/*! license */\n",
        "// attached\n",
        "const before=1; /* trailing */\n",
        "const before=1;\n/* attached */\n",
        "/* detached */\r\n\r\n/* attached */\r\n",
    ] {
        let source =
            format!("{prefix}import value from './dep.js';\nexport const result=value();\n");
        for (module_kind, module) in [(ModuleKind::Amd, "AMD"), (ModuleKind::System, "System")] {
            for maps in [false, true] {
                let id = "memory:///main.ts";
                let result = compile(
                    id,
                    &MapLoader::from([
                        ModuleSource::new(id, &source),
                        ModuleSource::new(
                            "memory:///dep.js",
                            "export default function value(){return 40;}",
                        ),
                    ]),
                    CompilerOptions {
                        target: EcmaTarget::Es2022,
                        module_kind,
                        source_map: maps,
                        es_module_interop: true,
                        ..CompilerOptions::default()
                    },
                );
                assert!(!result.has_errors(), "{:?}", result.diagnostics);
                rows.push(serde_json::json!({"source":source,"module":module,
                    "code":result.output.unwrap().artifacts[id].javascript}));
            }
        }
    }
    let script = r#"const fs=require('node:fs'),path=require('node:path'),assert=require('node:assert/strict');
const root=process.argv[1],ts=require(path.join(root,'backend/bluets/tests/fixtures/oracle_support/load_typescript.cjs'))(process.env.BLUEICE_BLUETSC_ORACLE||'tsc');
const rows=JSON.parse(fs.readFileSync(0,'utf8'));
for(const row of rows){const native=ts.transpileModule(row.source,{fileName:'main.ts',compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind[row.module],esModuleInterop:true}}).outputText;
for(const text of ['/* attached */','/* detached */','/*! license */','// attached','/* trailing */'])assert.equal(row.code.includes(text),native.includes(text),'original comment ownership '+row.module+' '+text+' '+row.source);}
console.log(JSON.stringify({nativeCommentPolicyProducts:rows.length,passed:true}));"#;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = Command::new("node")
        .args(["-e", script])
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
