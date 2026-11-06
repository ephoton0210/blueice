// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Capture the complete adjacent matrices before their independent Native role replay.
#[allow(dead_code)]
#[path = "oracle_support/ge5_cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "oracle_support/ge5_declarations.rs"]
mod declarations;
#[allow(dead_code)]
#[path = "oracle_support/gd2_maps.rs"]
mod decoded;
#[allow(dead_code)]
#[path = "oracle_support/ge5_inputs.rs"]
mod inputs;
#[allow(dead_code)]
#[path = "oracle_support/ge5_javascript_maps.rs"]
mod javascript_maps;
#[path = "oracle_support/project_paths.rs"]
mod project_paths;

#[test]
fn system_legacy_construction_keeps_its_exact_native_logical_endpoint() {
    inputs::verify_authorities();
    let temporary = cli::Temporary::new("system-legacy-logical-end");
    let mut rows = Vec::new();
    for index in [4652usize, 4653] {
        let specification = &inputs::input()["cases"][index];
        let original = inputs::resolve(specification);
        let actual = cli::observe_builds(&temporary, index, &original, 2);
        let failures = declarations::compare(
            specification,
            &original,
            &inputs::reference()["cases"][index],
            &actual,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        rows.push(
            serde_json::json!({"index":index,"id":specification["id"],"outputs":actual.outputs}),
        );
    }
    let script=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../development/browser_core/phase-18-bluets/tools/verify_ge5_system_legacy_endpoint.cjs");
    let mut child = std::process::Command::new("node")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&rows).unwrap())
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
