// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Template calls retain getter, string-hint conversion and suspension order.
use blueice_bluets::{compile, CompilerOptions, EcmaTarget, MapLoader, ModuleKind, ModuleSource};
use serde_json::Value;
use std::{fs, path::Path, process::Command};

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn template_suspensions_match_native_getter_conversion_and_abrupt_completion_order() {
    let input: Value = serde_json::from_str(include_str!(
        "../../bluets/tests/fixtures/ge1_downlevel/cases.json"
    ))
    .unwrap();
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("bluets-template-order-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    for form in ["template-await-order", "template-yield-order"] {
        let row = input["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["form"] == form)
            .unwrap();
        let source = row["files"]["main.ts"].as_str().unwrap();
        let result = compile(
            "memory:///main.ts",
            &MapLoader::from([ModuleSource::new("memory:///main.ts", source)]),
            CompilerOptions {
                target: EcmaTarget::Es5,
                libraries: Some(vec![EcmaTarget::Es2020]),
                module_kind: ModuleKind::CommonJs,
                ..CompilerOptions::default()
            },
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        let directory = root.join(form);
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("main.ts"), source).unwrap();
        fs::write(
            directory.join("blue.js"),
            &result.output.unwrap().artifacts["memory:///main.ts"].javascript,
        )
        .unwrap();
    }
    let script = r#"
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict');
const root = process.argv[1], ts = require(process.argv[2] + '/tests/fixtures/oracle_support/load_typescript.cjs')(process.env.BLUEICE_BLUETSC_ORACLE);
assert.equal(ts.version, '5.9.3');
const bootstrap = `
const originalConcat = String.prototype.concat;
Object.defineProperty(String.prototype, 'concat', {configurable:true, get() {
  events.push(['get', String(this)]);
  return function() { events.push(['call', String(this), arguments.length]); return Reflect.apply(originalConcat, this, arguments); };
}});
`;
(async () => {
for (const form of ['template-await-order','template-yield-order']) {
  const directory = path.join(root, form), program = ts.createProgram([path.join(directory,'main.ts')], {target:ts.ScriptTarget.ES5,module:ts.ModuleKind.CommonJS,lib:['lib.es2020.d.ts'],strict:true,skipLibCheck:true,noEmitOnError:true,outDir:path.join(directory,'native')});
  assert.equal(ts.getPreEmitDiagnostics(program).length,0); assert.equal(program.emit().emitSkipped,false);
  for (const filename of ['native/main.js','blue.js']) {
    const events = [], exports = {}, failure = new Error('template abrupt completion');
    class TrackedPromise extends Promise { static resolve(value) {
      if (value === 'x' || value === 'y') {
        events.push(['resolve',value]);
        const original = value; value = {[Symbol.toPrimitive](hint) { events.push(['convert',original,hint]); return original; }};
      }
      return Promise.resolve(value);
    }}
    const context = vm.createContext({events,exports,Promise:TrackedPromise});
    vm.runInContext(bootstrap,context);
    const code = fs.readFileSync(path.join(directory,filename),'utf8');
    vm.runInContext(code + (form.includes('yield') ? '\nexports.sequence = sequence;' : ''),context);
    const label = form + '/' + filename;
    if (form.includes('await')) {
      assert.equal(await exports.result, 'axby', label);
      assert.deepEqual(JSON.parse(JSON.stringify(events)),[['get','a'],['resolve','x'],['call','a',2],['convert','x','string'],['get','axb'],['resolve','y'],['call','axb',1],['convert','y','string']],label);
    } else {
      assert.equal(exports.result,'[{"value":1,"done":false},{"value":2,"done":false},{"value":"axby","done":false},{"value":"done","done":true}]',label);
      assert.deepEqual(JSON.parse(JSON.stringify(events)),[['get','a'],['call','a',2],['get','axb'],['call','axb',1]],label);
      for (const action of ['return','throw']) {
        events.length = 0; const iterator = exports.sequence();
        assert.equal(iterator.next().value,1);
        if(action === 'return') assert.deepEqual(JSON.parse(JSON.stringify(iterator.return('stop'))),{value:'stop',done:true});
        else assert.throws(() => iterator.throw(failure), error => error === failure);
        assert.deepEqual(JSON.parse(JSON.stringify(events)),[['get','a']],label + ': ' + action);
      }
    }
  }
}
})().catch(error => {console.error(error); process.exitCode=1;});
"#;
    let output = Command::new("node")
        .args(["-e", script])
        .arg(&root)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../bluets"))
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
