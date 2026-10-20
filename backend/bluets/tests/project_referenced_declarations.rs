// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.1.4: project references consume emitted declarations at the CLI boundary.

use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "oracle_support/project_paths.rs"]
mod project_paths;

const MATRIX: &str = include_str!("fixtures/project_referenced_declarations/rebuild-matrix.tsv");
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    step: String,
    built: Vec<String>,
}

impl Snapshot {
    fn row(&self) -> String {
        format!("{}\t{}\n", self.step, self.built.join(";"))
    }
}

fn matrix() -> Vec<Snapshot> {
    MATRIX
        .lines()
        .map(|line| {
            let (step, built) = line.split_once('\t').expect("step and rebuilt projects");
            Snapshot {
                step: step.into(),
                built: built.split(';').map(str::to_string).collect(),
            }
        })
        .collect()
}

fn rebuilt_projects(output: &Output, root: &Path, native: bool) -> Vec<String> {
    report(output)
        .lines()
        .filter_map(|line| {
            if native {
                line.strip_prefix("Building project: ")
            } else {
                line.split("Building project '")
                    .nth(1)
                    .and_then(|rest| rest.split('\'').next())
            }
        })
        .map(|project| project_paths::relative_to(root, project))
        .collect()
}

fn snapshot(step: &str, output: &Output, root: &Path, native: bool) -> Snapshot {
    Snapshot {
        step: step.into(),
        built: rebuilt_projects(output, root, native),
    }
}

fn temporary(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!(
        "bluets-projdecl-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(path.join("dep")).unwrap();
    fs::canonicalize(path).unwrap()
}

fn materialize(root: &Path) {
    let dependency = json!({
        "compilerOptions": {"composite": true, "declaration": true, "module": "commonjs"},
        "files": ["value.ts"]
    });
    let dependent = json!({
        "compilerOptions": {"composite": true, "declaration": true, "module": "commonjs"},
        "files": ["main.ts"],
        "references": [{"path": "./dep"}]
    });
    fs::write(
        root.join("dep/tsconfig.json"),
        serde_json::to_string_pretty(&dependency).unwrap() + "\n",
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_string_pretty(&dependent).unwrap() + "\n",
    )
    .unwrap();
    fs::write(
        root.join("dep/value.ts"),
        "export const value = 'source';\n",
    )
    .unwrap();
    fs::write(
        root.join("main.ts"),
        "import { value } from './dep/value';\nexport const result: string = value;\n",
    )
    .unwrap();
}

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn build(executable: &Path, root: &Path) -> Output {
    Command::new(executable)
        .current_dir(root)
        .args(["--build", "tsconfig.json", "--verbose", "--pretty", "false"])
        .output()
        .unwrap()
}

fn assert_runtime_value(root: &Path) {
    let result = Command::new("node")
        .arg("-e")
        .arg("process.stdout.write(JSON.stringify(require(process.argv[1]).result))")
        .arg(root.join("main.js"))
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", report(&result));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "\"source\"");
}

fn exercise(executable: &Path, label: &str, native: bool) -> Vec<Snapshot> {
    let root = temporary(label);
    materialize(&root);
    let first = build(executable, &root);
    assert!(first.status.success(), "cold: {}", report(&first));
    assert_runtime_value(&root);
    assert!(root.join("dep/value.d.ts").is_file());
    fs::write(
        root.join("dep/value.d.ts"),
        "export declare const value: number;\n",
    )
    .unwrap();
    fs::write(
        root.join("main.ts"),
        "import { value } from './dep/value';\nexport const result: number = value;\n",
    )
    .unwrap();
    let declaration = build(executable, &root);
    assert!(
        declaration.status.success(),
        "declaration differs from source: {}",
        report(&declaration)
    );
    assert!(fs::read_to_string(root.join("main.d.ts"))
        .unwrap()
        .contains("result: number"));
    assert_runtime_value(&root);
    let reports = vec![
        snapshot("projdecl-default-cold", &first, &root, native),
        snapshot(
            "projdecl-default-edited-declaration",
            &declaration,
            &root,
            native,
        ),
    ];
    fs::remove_dir_all(root).unwrap();
    reports
}

#[test]
#[ignore = "requires the pinned TypeScript 5.9.3 compiler"]
fn pinned_typescript_consumes_referenced_declarations() {
    let path = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let version = Command::new(&path).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let mut observed = exercise(&path, "typescript", false);
    observed.extend(custom_output_layout(&path, "layout-oracle-matrix", false));
    if env::var("BLUEICE_WRITE_PROJECT_DECLARATION_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_referenced_declarations/rebuild-matrix.tsv");
        fs::write(path, observed.iter().map(Snapshot::row).collect::<String>()).unwrap();
    } else {
        assert_eq!(observed, matrix());
    }
}

#[test]
fn public_build_consumes_referenced_declarations() {
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let mut observed = exercise(bluetsc, "bluetsc", true);
    observed.extend(custom_output_layout(bluetsc, "layout-bluetsc-matrix", true));
    assert_eq!(observed, matrix());
}

fn source_error_stays_in_dependency(executable: &Path, label: &str) {
    let root = temporary(label);
    materialize(&root);
    let initial = build(executable, &root);
    assert!(initial.status.success(), "{}", report(&initial));
    fs::write(
        root.join("dep/value.ts"),
        "export const value: string = 42;\n",
    )
    .unwrap();
    let failed = build(executable, &root);
    let diagnostics = report(&failed);
    assert!(!failed.status.success(), "{diagnostics}");
    assert!(diagnostics.contains("value.ts"), "{diagnostics}");
    assert!(!diagnostics.contains("main.ts("), "{diagnostics}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires the pinned TypeScript 5.9.3 compiler"]
fn pinned_typescript_source_error_stays_in_dependency() {
    let path = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    source_error_stays_in_dependency(&path, "source-error-oracle");
}

#[test]
fn public_build_source_error_stays_in_dependency() {
    source_error_stays_in_dependency(
        Path::new(env!("CARGO_BIN_EXE_bluetsc")),
        "source-error-bluetsc",
    );
}

#[test]
#[ignore = "requires the pinned TypeScript 5.9.3 compiler"]
fn pinned_typescript_project_reports_missing_referenced_declaration() {
    let path = PathBuf::from(
        env::var_os("BLUEICE_BLUETSC_ORACLE")
            .expect("set BLUEICE_BLUETSC_ORACLE to pinned TypeScript 5.9.3"),
    );
    let root = temporary("missing-oracle");
    materialize(&root);
    let initial = build(&path, &root);
    assert!(initial.status.success(), "{}", report(&initial));
    fs::remove_file(root.join("dep/value.d.ts")).unwrap();
    let check = Command::new(&path)
        .current_dir(&root)
        .args(["--project", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(!check.status.success(), "{}", report(&check));
    assert!(
        report(&check).contains("error TS6305:"),
        "{}",
        report(&check)
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn public_build_reports_missing_referenced_declaration() {
    let root = temporary("missing-bluetsc");
    materialize(&root);
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let initial = build(bluetsc, &root);
    assert!(initial.status.success(), "{}", report(&initial));
    fs::remove_file(root.join("dep/value.d.ts")).unwrap();
    let rebuilt = build(bluetsc, &root);
    assert!(rebuilt.status.success(), "{}", report(&rebuilt));
    assert!(
        report(&rebuilt).contains("recorded build output is missing or unsafe"),
        "{}",
        report(&rebuilt)
    );
    assert!(root.join("dep/value.d.ts").is_file());
    fs::remove_dir_all(root).unwrap();
}

fn custom_output_layout(executable: &Path, label: &str, native: bool) -> Vec<Snapshot> {
    let root = temporary(label);
    fs::create_dir_all(root.join("dep/src")).unwrap();
    let dependency = json!({
        "compilerOptions": {
            "composite": true,
            "declaration": true,
            "module": "commonjs",
            "rootDir": "src",
            "outDir": "dist",
            "declarationDir": "types"
        },
        "files": ["src/value.ts"]
    });
    let dependent = json!({
        "compilerOptions": {"composite": true, "declaration": true, "module": "commonjs"},
        "files": ["main.ts"],
        "references": [{"path": "./dep"}]
    });
    fs::write(
        root.join("dep/tsconfig.json"),
        serde_json::to_string_pretty(&dependency).unwrap() + "\n",
    )
    .unwrap();
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_string_pretty(&dependent).unwrap() + "\n",
    )
    .unwrap();
    fs::write(
        root.join("dep/src/value.ts"),
        "export type Value = string;\n",
    )
    .unwrap();
    fs::write(
        root.join("main.ts"),
        "import type { Value } from './dep/src/value';\nexport const result: Value = 'source';\n",
    )
    .unwrap();
    let initial = build(executable, &root);
    assert!(initial.status.success(), "{}", report(&initial));
    let declaration = root.join("dep/types/value.d.ts");
    assert!(declaration.is_file());
    fs::write(&declaration, "export type Value = number;\n").unwrap();
    fs::write(
        root.join("main.ts"),
        "import type { Value } from './dep/src/value';\nexport const result: Value = 1;\n",
    )
    .unwrap();
    let rebuilt = build(executable, &root);
    assert!(rebuilt.status.success(), "{}", report(&rebuilt));
    let result = Command::new("node")
        .arg("-e")
        .arg("process.stdout.write(JSON.stringify(require(process.argv[1]).result))")
        .arg(root.join("main.js"))
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", report(&result));
    assert_eq!(String::from_utf8_lossy(&result.stdout), "1");
    let observed = vec![
        snapshot("projdecl-layout-cold", &initial, &root, native),
        snapshot(
            "projdecl-layout-edited-declaration",
            &rebuilt,
            &root,
            native,
        ),
    ];
    fs::remove_dir_all(root).unwrap();
    observed
}

#[test]
#[ignore = "requires the pinned TypeScript 5.9.3 compiler"]
fn pinned_typescript_uses_custom_referenced_declaration_layout() {
    let path = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let observed = custom_output_layout(&path, "layout-oracle", false);
    assert_eq!(observed, matrix()[2..]);
}

#[test]
fn public_build_uses_custom_referenced_declaration_layout() {
    let observed = custom_output_layout(
        Path::new(env!("CARGO_BIN_EXE_bluetsc")),
        "layout-bluetsc",
        true,
    );
    assert_eq!(observed, matrix()[2..]);
}

#[test]
fn declaration_only_reference_never_authorizes_a_value_import() {
    let root = temporary("declaration-only");
    materialize(&root);
    let dependency = json!({
        "compilerOptions": {
            "composite": true,
            "declaration": true,
            "emitDeclarationOnly": true,
            "module": "commonjs"
        },
        "files": ["value.ts"]
    });
    fs::write(
        root.join("dep/tsconfig.json"),
        serde_json::to_string_pretty(&dependency).unwrap() + "\n",
    )
    .unwrap();
    fs::write(
        root.join("dep/value.ts"),
        "export const value = 1;\nexport type Value = number;\n",
    )
    .unwrap();
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let refused = build(bluetsc, &root);
    assert!(!refused.status.success(), "{}", report(&refused));
    assert!(report(&refused).contains("BTS2002"), "{}", report(&refused));
    assert!(!root.join("dep/value.js").exists());
    fs::write(
        root.join("main.ts"),
        "import type { Value } from './dep/value';\nexport const result: Value = 1;\n",
    )
    .unwrap();
    let accepted = build(bluetsc, &root);
    assert!(accepted.status.success(), "{}", report(&accepted));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_referenced_declaration_cannot_escape_the_owner_root() {
    use std::os::unix::fs::symlink;

    let root = temporary("symlink-output");
    materialize(&root);
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let initial = build(bluetsc, &root);
    assert!(initial.status.success(), "{}", report(&initial));
    let external = env::temp_dir().join(format!(
        "bluets-projdecl-external-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&external, "outside-marker\n").unwrap();
    fs::remove_file(root.join("dep/value.d.ts")).unwrap();
    symlink(&external, root.join("dep/value.d.ts")).unwrap();
    let denied = build(bluetsc, &root);
    assert!(!denied.status.success(), "{}", report(&denied));
    assert!(
        report(&denied).contains("outside") || report(&denied).contains("unsafe"),
        "{}",
        report(&denied)
    );
    assert_eq!(fs::read_to_string(&external).unwrap(), "outside-marker\n");
    fs::remove_dir_all(root).unwrap();
    fs::remove_file(external).unwrap();
}

#[test]
fn legacy_build_state_rebuilds_before_references_are_reused() {
    let root = temporary("legacy-state");
    materialize(&root);
    let bluetsc = Path::new(env!("CARGO_BIN_EXE_bluetsc"));
    let initial = build(bluetsc, &root);
    assert!(initial.status.success(), "{}", report(&initial));
    let path = root.join("dep/tsconfig.bluetsc.tsbuildinfo");
    let mut state: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state["version"] = json!(1);
    state.as_object_mut().unwrap().remove("declarations");
    state.as_object_mut().unwrap().remove("runtimes");
    fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
    let rebuilt = build(bluetsc, &root);
    assert!(rebuilt.status.success(), "{}", report(&rebuilt));
    assert!(
        report(&rebuilt).contains("Building project: dep/tsconfig.json"),
        "{}",
        report(&rebuilt)
    );
    let updated: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(updated["version"], 10);
    fs::remove_dir_all(root).unwrap();
}
