// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete ordered token roles for retained JavaScript, using only Native authority.
use super::{cli, decoded, inputs, javascript_maps, node_roles};
use blueice_bluets::{lex, TokenKind};
use serde_json::{json, Value};

fn token_roles(javascript: &str, map: &Value) -> Vec<Value> {
    let segments = map["segments"].as_array().unwrap();
    lex("<emitted>", javascript)
        .unwrap()
        .iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .map(|token| {
            let prefix = &javascript[..token.start];
            let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
            let column = prefix.rsplit('\n').next().unwrap().encode_utf16().count();
            let segment = segments.iter().rfind(|segment| {
                segment[0].as_u64().unwrap() as usize == line
                    && segment[1].as_u64().unwrap() as usize <= column
            });
            let original = segment
                .filter(|segment| segment.as_array().unwrap().len() > 2)
                .map(|segment| &segment.as_array().unwrap()[2..]);
            json!([token.text, original])
        })
        .collect()
}

const RETAINED_FORMS: &[&str] = &[
    "gd2_declaration_maps/inferred-record",
    "gd2_declaration_maps/inferred-return",
    "gd2_declaration_maps/inferred-const-tuple",
    "gd2_declaration_maps/record-union-annotation",
    "gd2_declaration_maps/interface-generic",
    "gd2_declaration_maps/alias-composite",
    "gd2_declaration_maps/generic-function",
    "gd2_declaration_maps/parameter-default-rest",
    "regression/function-body-call",
    "regression/function-record-return",
    "regression/function-callable-return",
    "regression/function-generic-callable-constraint",
    "regression/function-generic-record-return",
    "regression/function-tuple-callable-return",
    "regression/function-default-call",
];

#[test]
fn retained_commonjs_bindings_and_signatures_preserve_every_native_source_role() {
    compare(RETAINED_FORMS, 137);
}

#[test]
fn enum_scaffolding_and_members_preserve_every_native_source_role() {
    compare(
        &[
            "regression/enum-implicit-numeric",
            "regression/enum-initialized-numeric",
            "regression/enum-explicit-strings",
        ],
        21,
    );
}

#[test]
fn retained_class_members_and_export_assignments_preserve_every_native_source_role() {
    compare(
        &[
            "gd2_declaration_maps/class-fields",
            "gd2_declaration_maps/class-generic",
            "gd2_declaration_maps/class-accessors",
            "gd2_declaration_maps/class-interface-merge",
        ],
        44,
    );
}

#[test]
fn namespace_wrappers_merges_and_members_preserve_every_native_source_role() {
    compare(
        &[
            "gd2_declaration_maps/namespace-nested",
            "gd2_declaration_maps/namespace-merging",
            "gd2_declaration_maps/function-namespace-merge",
            "regression/enum-in-namespace",
        ],
        40,
    );
}

#[test]
fn retained_esm_forms_preserve_every_native_source_role() {
    let forms = RETAINED_FORMS
        .iter()
        .copied()
        .chain([
            "regression/enum-implicit-numeric",
            "regression/enum-initialized-numeric",
            "regression/enum-explicit-strings",
            "gd2_declaration_maps/class-fields",
            "gd2_declaration_maps/class-generic",
            "gd2_declaration_maps/class-accessors",
            "gd2_declaration_maps/class-interface-merge",
            "gd2_declaration_maps/namespace-nested",
            "gd2_declaration_maps/namespace-merging",
            "gd2_declaration_maps/function-namespace-merge",
            "regression/enum-in-namespace",
        ])
        .collect::<Vec<_>>();
    compare_module(&forms, 164, "ESNext");
}

#[test]
fn all_non_scalar_declarations_and_signature_regressions_keep_native_javascript_roles() {
    let forms = inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["origin"]["suite"] == "gd2_declaration_maps" || row.get("regression").is_some()
        })
        .map(|row| row["form"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(forms.len(), 43);
    let (mut failures, mut verified) = observe_module_report(&forms, 373, "CommonJS");
    let (other_failures, other_verified) = observe_module_report(&forms, 244, "ESNext");
    failures.extend(other_failures);
    verified.extend(other_verified);
    assert!(
        failures.is_empty(),
        "{} complete retained role/gap differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_REPORT_DIRECTORY") {
        let report = json!({"schema":1,"contract":"retained43 complete token UTF-16 owners and ordered gaps",
            "configurations":617,"forms":forms,"repeatBuilds":2,"completeFamily":true,
            "inputSha256":inputs::sha(include_bytes!("../fixtures/ge5_source_maps/cases.json")),
            "compilerSha256":inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap()),
            "verified":verified,"failures":failures});
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join("retained-declarations43.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
}

fn compare(forms: &[&str], expected_configurations: usize) {
    compare_module(forms, expected_configurations, "CommonJS");
}

fn compare_module(forms: &[&str], expected_configurations: usize, module: &str) {
    let failures = observe_module(forms, expected_configurations, module);
    assert!(
        failures.is_empty(),
        "{} retained JavaScript role differences:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn observe_module(forms: &[&str], expected_configurations: usize, module: &str) -> Vec<String> {
    observe_module_report(forms, expected_configurations, module).0
}

fn observe_module_report(
    forms: &[&str],
    expected_configurations: usize,
    module: &str,
) -> (Vec<String>, Vec<Value>) {
    let mut verified = Vec::new();
    let temporary = cli::Temporary::new("javascript-retained-roles");
    let native = inputs::reference()["cases"].as_array().unwrap();
    let mut failures = Vec::new();
    let mut configurations = 0;
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if !forms.contains(&specification["form"].as_str().unwrap()) {
            continue;
        }
        let row = inputs::resolve(specification);
        if row["options"]["module"] != module {
            continue;
        }
        configurations += 1;
        let actual = cli::observe_builds(&temporary, index, &row, 2);
        assert_eq!(
            actual.success,
            native[index]["diagnostics"].as_array().unwrap().is_empty(),
            "{}",
            specification["id"]
        );
        assert_eq!(
            actual.diagnostics, native[index]["diagnostics"],
            "{}",
            specification["id"]
        );
        let map_failures = javascript_maps::compare(specification, &row, &native[index], &actual);
        assert!(map_failures.is_empty(), "{}", map_failures.join("\n"));
        for (name, expected) in native[index]["maps"].as_object().unwrap() {
            if expected["kind"] != "javascript" {
                continue;
            }
            let output = expected["output"].as_str().unwrap();
            let native_js = inputs::blob(native[index]["outputs"][output].as_str().unwrap());
            let native_raw = javascript_maps::native_map(&native[index], name);
            let native_map = decoded::decoded(native_raw);
            let map_text = node_roles::actual_map_text(&actual, name, output);
            let actual_map = decoded::decoded(&map_text);
            assert_eq!(actual_map["metadata"], native_map["metadata"]);
            let wanted = token_roles(native_js, &native_map);
            let observed = token_roles(&actual.outputs[output], &actual_map);
            let sources = row["files"]
                .as_object()
                .unwrap()
                .values()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            let compared = node_roles::compare(
                native_js,
                &native_map,
                &actual.outputs[output],
                &actual_map,
                &sources,
            );
            if let Err(error) = &compared {
                failures.push(format!(
                    "{}: {name}: complete token interiors/trivia: {error}",
                    specification["id"]
                ));
            }
            if wanted != observed {
                let first = wanted
                    .iter()
                    .zip(&observed)
                    .position(|(a, b)| a != b)
                    .unwrap_or(wanted.len().min(observed.len()));
                failures.push(format!("{}: {name}; complete mapped/unmapped token roles {}/{}; first {first}: Native {:?}, BlueTSC {:?}",
                    specification["id"], observed.len(), wanted.len(), wanted.get(first), observed.get(first)));
            }
            if compared.is_ok() && wanted == observed {
                verified.push(json!({"index":index,"id":specification["id"],"map":name,
                    "allCharactersClassified":true,
                    "nativeJavaScriptSha256":native[index]["outputs"][output],
                    "nativeMapSha256":inputs::sha(native_raw.as_bytes()),
                    "actualJavaScriptSha256":inputs::sha(actual.outputs[output].as_bytes()),
                    "actualMapSha256":inputs::sha(map_text.as_bytes())}));
            }
        }
    }
    assert_eq!(configurations, expected_configurations);
    (failures, verified)
}
