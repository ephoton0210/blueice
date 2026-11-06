// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Every frozen product of a preserved decorator family uses two public builds.
use super::{cli, declarations, inputs};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and the complete frozen preserved-decorator family"]
fn private_super_method_preserves_every_native_module_character_and_role() {
    observe("private-super-method");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and the complete frozen preserved-decorator family"]
fn private_super_accessors_preserve_every_native_module_character_and_role() {
    observe("private-super-accessors");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and the complete frozen preserved-decorator family"]
fn private_static_super_preserves_every_native_module_character_and_role() {
    observe("private-static-super");
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and the complete frozen preserved-decorator family"]
fn private_super_async_generator_preserves_every_native_module_character_and_role() {
    observe("private-super-async-generator");
}

fn observe(family: &str) {
    inputs::verify_authorities();
    let temporary = cli::Temporary::new("complete-preserved-decorator");
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let mut rows = Vec::new();
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if specification["form"] != format!("ge3_decorator_compositions/{family}") {
            continue;
        }
        let original = inputs::resolve(specification);
        let actual = cli::observe_builds(&temporary, index, &original, 2);
        let differences = declarations::compare(
            specification,
            &original,
            &inputs::reference()["cases"][index],
            &actual,
        );
        assert!(differences.is_empty(), "{}", differences.join("\n"));
        rows.push(
            json!({"index":index,"id":specification["id"],"compilerSha256":compiler,
            "repeatBuilds":2,"options":original["options"],"outputs":actual.outputs}),
        );
    }
    assert_eq!(rows.len(), 7);
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("decorator-{family}.json")),
            serde_json::to_vec(&rows).unwrap(),
        )
        .unwrap();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_decorator_preserved_roles.cjs",
    );
    let mut child = Command::new("node")
        .arg(script)
        .arg(family)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let written = child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap());
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    written.unwrap();
    eprintln!("{}", String::from_utf8_lossy(&result.stdout));
}
