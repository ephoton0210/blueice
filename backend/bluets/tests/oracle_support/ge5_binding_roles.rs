// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Binding families keep their complete frozen product inventory.
use super::{cli, declarations, inputs};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for complete Native binding roles"]
fn array_nested_binding_all_frozen_products() {
    family("array-nested-binding", 51);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for complete Native binding roles"]
fn object_nested_binding_all_frozen_products() {
    family("object-nested-binding", 27);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for complete Native binding roles"]
fn mixed_nested_default_all_frozen_products() {
    family("mixed-nested-default", 27);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for complete Native binding roles"]
fn nested_parameter_all_frozen_products() {
    family("nested-parameter", 27);
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 for complete Native binding roles"]
fn binding_iterator_finally_all_frozen_products() {
    family("binding-iterator-finally", 27);
}

fn family(form: &str, count: usize) {
    inputs::verify_authorities();
    let temporary = cli::Temporary::new(form);
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let mut observations = Vec::new();
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if specification["form"] != format!("ge1_downlevel/{form}") {
            continue;
        }
        let row = inputs::resolve(specification);
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        let failures = declarations::compare(
            specification,
            &row,
            &inputs::reference()["cases"][index],
            &actual,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        observations.push(
            json!({"index":index,"id":specification["id"],"compilerSha256":compiler,
            "repeatBuilds":2,"options":row["options"],"outputs":actual.outputs}),
        );
    }
    assert_eq!(observations.len(), count);
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            Path::new(&directory).join(format!("binding-{form}.json")),
            serde_json::to_vec(&observations).unwrap(),
        )
        .unwrap();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_binding_roles.cjs");
    let mut child = Command::new("node")
        .arg("--experimental-vm-modules")
        .arg(script)
        .arg(form)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&observations).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}

#[test]
fn binding_names_initializers_and_declaration_completions_follow_native_boundaries() {
    use blueice_bluets::{lex, TokenKind};
    use serde_json::Value;
    fn owner(code: &str, map: &Value, offset: usize) -> Option<Value> {
        let prefix = &code[..offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
        let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
        map["segments"]
            .as_array()
            .unwrap()
            .iter()
            .rfind(|segment| segment[0] == line && segment[1].as_u64().unwrap() as usize <= column)
            .filter(|segment| segment.as_array().unwrap().len() > 2)
            .map(|segment| json!(&segment.as_array().unwrap()[2..]))
    }
    let temporary = cli::Temporary::new("binding-boundaries");
    let mut count = 0;
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let form = specification["form"].as_str().unwrap();
        if ![
            "ge1_downlevel/array-nested-binding",
            "ge1_downlevel/object-nested-binding",
            "ge1_downlevel/mixed-nested-default",
        ]
        .contains(&form)
            || specification["profile"] != "external"
        {
            continue;
        }
        let row = inputs::resolve(specification);
        if row["options"]["target"] != "ES5" {
            continue;
        }
        let expected = &inputs::reference()["cases"][index];
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert!(declarations::compare(specification, &row, expected, &actual).is_empty());
        let native_code = inputs::blob(expected["outputs"]["out/main.js"].as_str().unwrap());
        let native_map = super::decoded::decoded(inputs::blob(
            expected["outputs"]["out/main.js.map"].as_str().unwrap(),
        ));
        let actual_code = &actual.outputs["out/main.js"];
        let actual_map = super::decoded::decoded(&actual.outputs["out/main.js.map"]);
        let wanted = ["head", "tail", "value", "40"];
        for name in wanted {
            let roles = |code: &str, map: &Value| {
                lex("<javascript>", code)
                    .unwrap()
                    .iter()
                    .filter(|token| token.kind != TokenKind::Eof && token.text == name)
                    .filter_map(|token| {
                        owner(code, map, token.start).map(|_| {
                            code[token.start..token.end]
                                .char_indices()
                                .map(|(offset, _)| owner(code, map, token.start + offset))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                roles(actual_code, &actual_map),
                roles(native_code, &native_map),
                "{}: complete source name/literal interiors",
                specification["id"]
            );
        }
        let completion = |code: &str, map: &Value| {
            lex("<javascript>", code)
                .unwrap()
                .iter()
                .find(|token| token.is(";") && owner(code, map, token.start).is_some())
                .and_then(|token| owner(code, map, token.start))
        };
        assert_eq!(
            completion(actual_code, &actual_map),
            completion(native_code, &native_map),
            "{}: declaration completion",
            specification["id"]
        );
        count += 1;
    }
    assert_eq!(count, 16);
}
