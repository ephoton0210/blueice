// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! System re-export forwarding retains the pinned generated-transport boundary.
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
fn system_star_export_transport_cannot_acquire_original_source() {
    inputs::verify_authorities();
    let temporary = cli::Temporary::new("interop-system-star");
    let no_maps = cli::Temporary::new("interop-system-star-no-maps");
    let compiler = inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap());
    let mut rows = Vec::new();
    for (index, specification) in inputs::input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        if specification["form"] != "ge2_module_helpers/interop-exportstar-cycle" {
            continue;
        }
        let original = inputs::resolve(specification);
        if original["options"]["module"] != "System" {
            continue;
        }
        let actual = cli::observe_builds(&temporary, index, &original, 2);
        let failures = declarations::compare(
            specification,
            &original,
            &inputs::reference()["cases"][index],
            &actual,
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        let mut plain = original.clone();
        let options = plain["options"].as_object_mut().unwrap();
        for flag in [
            "sourceMap",
            "inlineSourceMap",
            "inlineSources",
            "declarationMap",
        ] {
            options.insert(flag.to_string(), serde_json::json!(false));
        }
        options.remove("sourceRoot");
        options.remove("mapRoot");
        let without_maps = cli::observe_builds(&no_maps, index, &plain, 2);
        assert!(without_maps.success && without_maps.diagnostics == serde_json::json!([]));
        assert!(!without_maps
            .outputs
            .keys()
            .any(|name| name.ends_with(".map")));
        for (name, text) in &actual.outputs {
            if name.ends_with(".js") || name.ends_with(".d.ts") {
                assert_eq!(
                    without_publication(text),
                    without_publication(&without_maps.outputs[name]),
                    "{index} {name}: map settings preserve executable and declaration bytes"
                );
            }
        }
        rows.push(
            serde_json::json!({"index":index,"id":specification["id"],"outputs":actual.outputs,
                "compilerSha256":compiler,"repeatBuilds":2}),
        );
    }
    assert_eq!(rows.len(), 40);
    assert_eq!(
        compiler,
        inputs::sha(&std::fs::read(env!("CARGO_BIN_EXE_bluetsc")).unwrap())
    );
    if let Some(directory) = std::env::var_os("BLUEICE_GE5_ROLE_OBSERVATION_DIRECTORY") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("interop-system-star.json"),
            serde_json::to_vec(&rows).unwrap(),
        )
        .unwrap();
    }
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/verify_ge5_interop_system_star.cjs",
    );
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

fn without_publication(text: &str) -> &str {
    let text = text
        .rfind("\n//# sourceMappingURL=")
        .map_or(text, |at| &text[..at]);
    text.trim_end_matches(['\r', '\n'])
}
