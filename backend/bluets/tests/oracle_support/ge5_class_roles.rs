// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Independent class/super Native authority, all frozen products and both builds.
use super::{cli, declarations, decoded, inputs};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn owner(code: &str, map: &Value, offset: usize) -> Value {
    let prefix = &code[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
    map["segments"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|segment| {
            segment[0].as_u64().unwrap() as usize == line
                && segment[1].as_u64().unwrap() as usize <= column
        })
        .filter(|segment| segment.as_array().unwrap().len() > 2)
        .map_or(Value::Null, |segment| {
            json!(&segment.as_array().unwrap()[2..])
        })
}

#[test]
fn es5_class_retains_complete_constructor_signature_and_body_origins() {
    let temporary = cli::Temporary::new("class-retained-origins");
    for index in [264, 265] {
        let specification = &inputs::input()["cases"][index];
        let row = inputs::resolve(specification);
        let native = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert!(declarations::compare(specification, &row, native, &actual).is_empty());
        let name = "out/base.js";
        let wanted = inputs::blob(native["outputs"][name].as_str().unwrap());
        let observed = &actual.outputs[name];
        let wanted_map = decoded::decoded(inputs::blob(
            native["outputs"][format!("{name}.map")].as_str().unwrap(),
        ));
        let observed_map = decoded::decoded(&actual.outputs[&format!("{name}.map")]);
        let target = "function Base(value)";
        let n = wanted.find(target).unwrap();
        let a = observed.find(target).unwrap();
        for offset in 0..target.len() {
            assert_eq!(
                owner(observed, &observed_map, a + offset),
                owner(wanted, &wanted_map, n + offset),
                "constructor signature UTF-16 unit {offset}"
            );
        }
        let completion = "return Base;";
        let n_completion = wanted.find(completion).unwrap() + completion.len() - 1;
        let a_completion = observed.find(completion).unwrap() + completion.len() - 1;
        assert_eq!(
            owner(observed, &observed_map, a_completion),
            owner(wanted, &wanted_map, n_completion),
            "complete class return terminator"
        );
        let target = "this.value";
        let n = wanted.find(target).unwrap();
        let a = observed.find(target).unwrap();
        for offset in 0..target.len() {
            assert_eq!(
                owner(observed, &observed_map, a + offset),
                owner(wanted, &wanted_map, n + offset),
                "constructor body UTF-16 unit {offset}"
            );
        }
    }
}

#[test]
fn class_heritage_preserves_its_evaluation_and_receiver() {
    let temporary = cli::Temporary::new("class-heritage-execution");
    for index in [264, 265, 312, 313] {
        let specification = &inputs::input()["cases"][index];
        let row = inputs::resolve(specification);
        let expected = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert!(declarations::compare(specification, &row, expected, &actual).is_empty());
        let script = r#"const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const [native,actual]=JSON.parse(fs.readFileSync(0,'utf8'));
function execute(outputs) { const cache=new Map();function load(name) {if(cache.has(name))return cache.get(name);const exports={};cache.set(name,exports);const module={exports};vm.runInNewContext(outputs[name],{exports,module,require:id=>load('out/'+id.replace(/^\.\//,''))},{timeout:1000});return module.exports;}return load('out/main.js').result;}
assert.equal(execute(actual),execute(native));"#;
        let native = expected["outputs"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(name, _)| name.ends_with(".js"))
            .map(|(name, hash)| (name.clone(), json!(inputs::blob(hash.as_str().unwrap()))))
            .collect::<serde_json::Map<String, Value>>();
        let mut child = Command::new("node")
            .args(["-e", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&json!([native, actual.outputs])).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            specification["id"],
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn es5_complete_class_roles_retain_original_ast_boundaries() {
    let temporary = cli::Temporary::new("class-ast-boundaries");
    let mut rows = Vec::new();
    for index in [264, 265, 312, 313, 360, 361, 1000, 1001, 1040, 1041] {
        let specification = &inputs::input()["cases"][index];
        let row = inputs::resolve(specification);
        let native = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert!(declarations::compare(specification, &row, native, &actual).is_empty());
        rows.push(json!({"index": index, "outputs": actual.outputs}));
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_class_boundaries.cjs",
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

#[test]
fn decorated_class_wrapper_uses_original_module_context() {
    let temporary = cli::Temporary::new("class-system-decorated-binding");
    let mut rows = Vec::new();
    for index in [4492, 4493, 4572, 4573, 4612, 4613, 4652, 4653] {
        let specification = &inputs::input()["cases"][index];
        let row = inputs::resolve(specification);
        let native = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        let failures = declarations::compare(specification, &row, native, &actual);
        assert!(
            failures.is_empty(),
            "{}: {}",
            specification["id"],
            failures.join("\n")
        );
        rows.push(json!({"index": index, "outputs": actual.outputs}));
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_class_decorated_binding.cjs");
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

fn selected(specification: &Value) -> bool {
    matches!(
        specification["form"].as_str().unwrap(),
        "ge1_downlevel/called-imported-heritage"
            | "ge1_downlevel/indexed-heritage"
            | "ge1_downlevel/computed-constructor-spread"
            | "ge1_downlevel/constructor-and-super-spread"
            | "ge1_downlevel/computed-super-call-order"
            | "ge1_downlevel/super-write-receiver-order"
            | "ge2_module_helpers/inherit-constructor-spread"
    )
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3; all 661 frozen class configurations"]
fn all_class_super_targets_modules_providers_and_products_match_native_roles() {
    super::class_cli::run(selected, 661, "class-super", "verify_ge5_class_roles.cjs");
}
