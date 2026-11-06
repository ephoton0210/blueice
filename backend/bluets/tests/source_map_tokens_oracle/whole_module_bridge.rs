// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete K11 module proof after the original public API identity checks.

use super::{matrix::MatrixRow, ModuleKind, Value};
use ring::digest::{digest, SHA256};
use serde_json::json;
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) struct Blue<'a> {
    pub(super) code: &'a str,
    pub(super) map: &'a Value,
}

fn sha(source: &str) -> String {
    digest(&SHA256, source.as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn source_identity(map: &Value, source: &str, inline: bool) {
    let sources = map["sources"].as_array().unwrap();
    assert_eq!(
        sources.len(),
        1,
        "K11 whole proof needs one original source"
    );
    let basename = sources[0]
        .as_str()
        .unwrap()
        .rsplit(['/', '\\'])
        .next()
        .unwrap();
    assert_eq!(basename, "main.ts", "exact original source basename");
    assert_eq!(map["file"], "main.js", "exact emitted output basename");
    if inline {
        assert_eq!(map["sourcesContent"], json!([source]));
    } else {
        assert!(map.get("sourcesContent").is_none());
    }
}

pub(super) fn verify_rows(
    row: &MatrixRow<'_>,
    source: &str,
    native: &Path,
    native_without_inline: &Path,
    blue: [Blue<'_>; 2],
) {
    let (file, frozen) = match row.name {
        "k11-standard-method" => (
            "decorated.ts",
            "e242c3674ae8287d9df707a7501580043542b6e0912eaece9569586a890ddfd4",
        ),
        "k11-compacted-relocation" => (
            "decorated_parenthesized.ts",
            "b01d39fc18b5c3b9ed688293e06ef5341ba9ca1260c1ad97951f7a0487fca7ee",
        ),
        "k11-erased-relocation" => (
            "decorated_erased_assertion.ts",
            "f157e75862c138ecb9158af81dc81237311e1f7ba7e1dda47b972e47ffd2b96b",
        ),
        "k11-composed-decorators" => (
            "decorated_multiple.ts",
            "c20b6fe0d901114e100bcc1e0022b9613242e7247bc4a239903ab539a2936f9e",
        ),
        "k11-legacy-class-member" => (
            "legacy_decorated.ts",
            "6b5ea6445580bfbe55cb491dde83154b7689ae38c71fdda2753fa8acb819aba4",
        ),
        "k11-legacy-parameter-metadata" => (
            "legacy_parameter_metadata.ts",
            "84d4cad4d110aa257b3c00b90344e57390c4ddff460cf2997763b86cdea82d3f",
        ),
        "k11-system-wrapper" => (
            "system_wrapper.ts",
            "f5be563ed189d1c9ba0f5e231f097af4660533868a7bcd7f1030a7fd68d42b3f",
        ),
        _ => return,
    };
    assert_eq!(row.file, file);
    assert_eq!(row.line_endings, "lf");
    assert_eq!(row.target.as_str(), "es2022");
    let legacy = matches!(
        row.name,
        "k11-legacy-class-member" | "k11-legacy-parameter-metadata"
    );
    let system = row.name == "k11-system-wrapper";
    assert_eq!(
        row.module,
        if system {
            ModuleKind::System
        } else {
            ModuleKind::Esm
        }
    );
    assert_eq!(row.legacy, legacy);
    assert_eq!(row.metadata, row.name == "k11-legacy-parameter-metadata");
    assert_eq!(row.system_runner, system);
    assert_eq!(sha(source), frozen, "unchanged original K11 fixture bytes");
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../development/browser_core/phase-18-bluets/tools/verify_k11_whole_module.cjs");
    for (index, native) in [native, native_without_inline].into_iter().enumerate() {
        let inline = index == 0;
        let native_code = fs::read_to_string(native.join("main.js")).unwrap();
        let native_map: Value =
            serde_json::from_str(&fs::read_to_string(native.join("main.js.map")).unwrap()).unwrap();
        source_identity(&native_map, source, inline);
        source_identity(blue[index].map, source, inline);
        let input = json!({"schema":1,"case":row.name,"source":source,
            "options":{"target":"ES2022","module":if system { "System" } else { "ES2022" },"strict":true,
                "experimentalDecorators":row.legacy,"emitDecoratorMetadata":row.metadata,
                "useDefineForClassFields":true,"inlineSources":inline},
            "sourceIdentity":{"basename":"main.ts","sourceCount":1,"sourceSha256":sha(source)},
            "native":{"code":native_code,"map":native_map},
            "blue":{"code":blue[index].code,"map":blue[index].map}});
        let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
        let mut child = Command::new(node)
            .arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&input).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{} inlineSources={inline}: {}{}",
            row.name,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["case"], row.name);
        assert_eq!(report["sourceSha256"], frozen);
        assert_eq!(report["inlineSources"], inline);
        assert_eq!(report["completeModuleRoles"], true);
        assert!(report["effectiveRejectedControls"].as_u64().unwrap() > 0);
        eprintln!(
            "K11 complete module: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
