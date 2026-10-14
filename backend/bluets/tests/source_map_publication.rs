// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.11: source maps must survive external and inline public CLI publication.

use base64::Engine;
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const SOURCE: &str = include_str!("fixtures/k11_source_maps/basic.ts");
const MATRIX: &str = include_str!("fixtures/k11_source_maps/source-map-tokens-checker-matrix.tsv");
const SYSTEM_LOADER: &str = r#"
const fs = require('node:fs');
const vm = require('node:vm');
globalThis.System = { register(dependencies, declare) {
    if (dependencies.length !== 0) throw new Error('unexpected dependency');
    const exports = {};
    const body = declare((name, value) => {
        if (typeof name === 'object') Object.assign(exports, name);
        else exports[name] = value;
        return value;
    }, {id: 'main'});
    if (body.setters.length !== 0) throw new Error('unexpected setter');
    body.execute();
} };
const input = process.argv[1];
vm.runInThisContext(fs.readFileSync(input, 'utf8'), {filename: input});
"#;

fn map_from_output(javascript: &str, directory: &Path, inline: bool) -> Value {
    let url = javascript
        .lines()
        .find_map(|line| line.strip_prefix("//# sourceMappingURL="))
        .expect("sourceMappingURL trailer");
    if inline {
        let payload = url
            .strip_prefix("data:application/json;base64,")
            .expect("inline source-map data URL");
        assert!(
            !directory.join("main.js.map").exists(),
            "no external map in inline mode"
        );
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("valid base64 map");
        serde_json::from_slice(&bytes).expect("valid inline map JSON")
    } else {
        assert_eq!(url, "main.js.map");
        serde_json::from_str(&fs::read_to_string(directory.join("main.js.map")).unwrap()).unwrap()
    }
}

fn source_names(map: &Value) -> Vec<String> {
    map["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|source| {
            source
                .as_str()
                .unwrap()
                .rsplit(['/', '\\'])
                .next()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to pinned TypeScript 5.9.3"]
fn external_and_inline_source_maps_match_public_typescript_output() {
    let tsc = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "Version 5.9.3"
    );
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary = env::temp_dir().join(format!(
        "blueice-k11-cli-map-{}-{nonce}",
        std::process::id()
    ));
    let node = env::var_os("BLUEICE_NODE_ORACLE").unwrap_or_else(|| "node".into());
    let rows = MATRIX.lines().skip(1).collect::<Vec<_>>();
    assert_eq!(rows.len(), 11, "all K.11 matrix rows must reach the CLI");
    for row in rows {
        let fields = row.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 11, "invalid K.11 matrix row: {row}");
        let case = fields[0];
        assert_eq!(fields[1], "accept", "{case}: expected verdict");
        let mut source = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/k11_source_maps")
                .join(fields[2]),
        )
        .unwrap();
        if fields[3] == "crlf" {
            assert!(!source.contains('\r'));
            source = source.replace('\n', "\r\n");
        } else {
            assert_eq!(fields[3], "lf", "{case}: line endings");
        }
        let legacy = fields[6] == "true";
        let metadata = fields[7] == "true";
        assert!(matches!(fields[5], "ES2022" | "System"));
        let system = fields[10] == "system";
        assert_eq!(system, fields[5] == "System");
        for (inline, inline_sources) in [(false, false), (false, true), (true, false), (true, true)]
        {
            let mode = format!(
                "{}-sources-{}",
                if inline { "inline" } else { "external" },
                if inline_sources { "on" } else { "off" }
            );
            let roots = [
                temporary.join(case).join(&mode).join("native"),
                temporary.join(case).join(&mode).join("blue"),
            ];
            for root in &roots {
                fs::create_dir_all(root).unwrap();
                fs::write(root.join("main.ts"), &source).unwrap();
                fs::write(root.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
                let config = json!({
                    "compilerOptions": {
                        "target": fields[4],
                        "module": fields[5],
                        "strict": true,
                        "experimentalDecorators": legacy,
                        "emitDecoratorMetadata": metadata,
                        "sourceMap": !inline,
                        "inlineSourceMap": inline,
                        "inlineSources": inline_sources,
                        "outDir": "out"
                    },
                    "files": ["main.ts"]
                });
                fs::write(
                    root.join("tsconfig.json"),
                    serde_json::to_vec(&config).unwrap(),
                )
                .unwrap();
            }
            let native = Command::new(&tsc)
                .arg("--project")
                .arg(roots[0].join("tsconfig.json"))
                .output()
                .unwrap();
            assert!(
                native.status.success(),
                "{case}/{mode}: Native: {}",
                String::from_utf8_lossy(&native.stdout)
            );
            let blue = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
                .arg("--project")
                .arg(roots[1].join("tsconfig.json"))
                .output()
                .unwrap();
            assert!(
                blue.status.success(),
                "{case}/{mode}: BlueTSC: {}",
                String::from_utf8_lossy(&blue.stderr)
            );
            let native_out = roots[0].join("out");
            let blue_out = roots[1].join("out");
            let native_js = fs::read_to_string(native_out.join("main.js")).unwrap();
            let blue_js = fs::read_to_string(blue_out.join("main.js")).unwrap();
            let native_map = map_from_output(&native_js, &native_out, inline);
            let blue_map = map_from_output(&blue_js, &blue_out, inline);
            if inline_sources {
                assert_eq!(
                    blue_map["sourcesContent"],
                    json!([source.as_str()]),
                    "{case}/{mode}"
                );
            } else {
                assert!(
                    blue_map.get("sourcesContent").is_none(),
                    "{case}/{mode}: inlineSources disabled"
                );
            }
            for name in ["version", "file", "sourceRoot", "names", "sourcesContent"] {
                assert_eq!(blue_map[name], native_map[name], "{case}/{mode}: {name}");
            }
            assert_eq!(
                source_names(&blue_map),
                source_names(&native_map),
                "{case}/{mode}: sources"
            );
            let run = |path: &Path| {
                let mut command = Command::new(&node);
                if system {
                    command.arg("-e").arg(SYSTEM_LOADER);
                }
                command.arg(path).output().unwrap()
            };
            let native_run = run(&native_out.join("main.js"));
            let blue_run = run(&blue_out.join("main.js"));
            assert_eq!(
                blue_run.status, native_run.status,
                "{case}/{mode}: exit status"
            );
            assert_eq!(blue_run.stdout, native_run.stdout, "{case}/{mode}: stdout");
            assert_eq!(blue_run.stderr, native_run.stderr, "{case}/{mode}: stderr");
        }
    }
    fs::remove_dir_all(&temporary).unwrap();
}

#[test]
fn mutually_exclusive_map_modes_publish_nothing() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().join(format!(
        "blueice-k11-cli-map-conflict-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.ts"), SOURCE).unwrap();
    let config = json!({
        "compilerOptions": {
            "target": "ES2022",
            "module": "ES2022",
            "sourceMap": true,
            "inlineSourceMap": true,
            "outDir": "out"
        },
        "files": ["main.ts"]
    });
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let blue = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .arg("--project")
        .arg(root.join("tsconfig.json"))
        .output()
        .unwrap();
    assert!(!blue.status.success());
    assert!(
        String::from_utf8_lossy(&blue.stderr).contains("cannot be combined"),
        "{}",
        String::from_utf8_lossy(&blue.stderr)
    );
    assert!(
        !root.join("out").exists(),
        "conflicting map modes must not publish"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn managed_build_switches_inline_and_external_publication() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = env::temp_dir().join(format!(
        "blueice-k11-managed-map-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("main.ts"), SOURCE).unwrap();
    fs::write(root.join("package.json"), "{\"type\":\"module\"}\n").unwrap();
    let config_path = root.join("bluetsc.json");
    let out = root.join("out");
    for inline in [false, true, false] {
        let config = json!({
            "entries": ["main.ts"],
            "outDir": "out",
            "target": "es2022",
            "module": "esnext",
            "sourceMap": !inline,
            "inlineSourceMap": inline,
            "inlineSources": true
        });
        fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["build", "--config"])
            .arg(&config_path)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let javascript = fs::read_to_string(out.join("main.js")).unwrap();
        let map = map_from_output(&javascript, &out, inline);
        assert_eq!(map["version"], 3);
        assert_eq!(map["sourcesContent"], json!([SOURCE]));
        let manifest: Value =
            serde_json::from_str(&fs::read_to_string(out.join("bluetsc.manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["sourceMap"], true);
        assert_eq!(
            manifest.get("inlineSourceMap").and_then(Value::as_bool),
            inline.then_some(true)
        );
        let run = Command::new("node")
            .arg(out.join("main.js"))
            .output()
            .unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"3\n");
    }
    fs::remove_dir_all(root).unwrap();
}
