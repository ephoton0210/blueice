// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Only the bounded owner bootstrap may select a closed program's roots.

use blueice_ipc::compiler_catalog::{CompilerCatalogBootstrap, COMPILER_CATALOG_BOOTSTRAP_VERSION};
use serde_json::{json, Value};

fn catalog() -> Value {
    json!({"version":COMPILER_CATALOG_BOOTSTRAP_VERSION,"projects":[{
        "canonical_project_root":"project:///program",
        "canonical_config_root":"project:///program/tsconfig.json",
        "canonical_output_root":"project:///program-output",
        "entry_module":"project:///program/main.ts",
        "program_roots":["project:///program/main.ts","project:///program/consumer.ts"],
        "modules":[
            {"canonical_id":"project:///program/main.ts","text":"const value: number = 1;"},
            {"canonical_id":"project:///program/consumer.ts","text":"const answer: number = value;"},
            {"canonical_id":"project:///program/unselected.ts","text":"const value: string = 'unselected';"}
        ]
    }]})
}

fn parse(value: &Value) -> std::io::Result<CompilerCatalogBootstrap> {
    CompilerCatalogBootstrap::from_json_slice(&serde_json::to_vec(value).unwrap())
}

#[test]
fn an_owner_can_select_a_closed_subset_without_granting_query_or_write_authority() {
    let catalog = parse(&catalog()).unwrap();
    assert!(!catalog.projects[0].expose_to_compiler_ipc);
    assert!(!catalog.projects[0].grant_output_write);
    let value = serde_json::to_value(&catalog).unwrap();
    assert_eq!(
        value["projects"][0]["program_roots"],
        json!([
            "project:///program/main.ts",
            "project:///program/consumer.ts"
        ])
    );
    assert_eq!(catalog.projects[0].modules.len(), 3);
    let mut legacy = value;
    legacy["projects"][0]
        .as_object_mut()
        .unwrap()
        .remove("program_roots");
    assert!(
        parse(&legacy).is_ok(),
        "omitted roots retain the existing single-entry owner behavior"
    );
}

#[test]
fn invalid_selected_roots_are_rejected_as_a_complete_owner_transaction() {
    for roots in [
        json!([]),
        json!([""]),
        json!(["project:///program/bad\0.ts"]),
        json!(["project:///program/main.ts", "project:///program/main.ts"]),
        json!([
            "project:///program/main.ts",
            "project:///program/missing.ts"
        ]),
        json!(["project:///program/main.ts", "project:///outside.ts"]),
        json!([
            "project:///program/main.ts",
            "project:///program/../outside.ts"
        ]),
        json!([
            "project:///program/consumer.ts",
            "project:///program/main.ts"
        ]),
        json!("project:///program/main.ts"),
    ] {
        let mut input = catalog();
        input["projects"][0]["program_roots"] = roots.clone();
        assert!(
            parse(&input).is_err(),
            "accepted invalid owner roots {roots}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_selected_physical_root_cannot_enter_the_catalog_through_a_symlink() {
    use std::os::unix::fs::symlink;
    let directory = std::env::temp_dir().join(format!(
        "compiler-roots-policy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let project = directory.join("project");
    let output = directory.join("output");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&output).unwrap();
    let main = project.join("main.ts");
    let config = project.join("tsconfig.json");
    let consumer = project.join("consumer.ts");
    let alias = project.join("alias.ts");
    std::fs::write(&main, "const value = 1;").unwrap();
    std::fs::write(&consumer, "const answer = value;").unwrap();
    std::fs::write(&config, "{}").unwrap();
    symlink(&consumer, &alias).unwrap();
    let mut input = catalog();
    input["projects"][0] = json!({
        "canonical_project_root":project,"canonical_config_root":config,
        "canonical_output_root":output,"entry_module":main,"program_roots":[main,consumer],
        "modules":[{"canonical_id":main,"text":"const value = 1;"},
            {"canonical_id":consumer,"text":"const answer = value;"}]
    });
    assert!(
        parse(&input).is_ok(),
        "canonical positive control must pass first"
    );
    input["projects"][0]["program_roots"][1] = json!(alias);
    input["projects"][0]["modules"][1]["canonical_id"] = json!(alias);
    assert!(parse(&input).is_err());
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 0);
    std::fs::remove_dir_all(directory).unwrap();
}
