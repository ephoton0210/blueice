// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! K.12.2.2: observed filesystem inputs invalidate project build state.

use serde_json::json;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CASES: [&str; 5] = [
    "manifest",
    "nearer-package",
    "repointed-symlink",
    "paths-types",
    "unrelated",
];
const MATRIX: &str =
    include_str!("fixtures/project_build_invalidation/build-invalidation-matrix.tsv");

fn temporary(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!(
        "bluets-projinval-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(path.join("dep")).unwrap();
    fs::canonicalize(path).unwrap()
}

fn materialize(root: &Path, case: &str) {
    let dep = json!({
        "compilerOptions": {"composite": true, "declaration": true, "module": "commonjs"},
        "files": ["main.ts"]
    });
    let file = if case == "nearer-package" {
        "src/nested/main.ts"
    } else {
        "src/main.ts"
    };
    let mut config = json!({
        "compilerOptions": {
            "composite": true,
            "declaration": true,
            "module": "NodeNext",
            "moduleResolution": "NodeNext"
        },
        "files": [file],
        "references": [{"path": "./dep"}]
    });
    if case == "paths-types" {
        config["compilerOptions"]["baseUrl"] = json!(".");
        config["compilerOptions"]["paths"] = json!({"alias": ["types/one.d.ts"]});
        config["compilerOptions"]["typeRoots"] = json!(["./type_roots"]);
        config["compilerOptions"]["types"] = json!(["one"]);
        fs::create_dir_all(root.join("types")).unwrap();
        for name in ["one", "two"] {
            fs::write(
                root.join(format!("types/{name}.d.ts")),
                "export interface Marker { id: number }\n",
            )
            .unwrap();
            fs::create_dir_all(root.join(format!("type_roots/{name}"))).unwrap();
            fs::write(
                root.join(format!("type_roots/{name}/index.d.ts")),
                "declare type GlobalMarker = number;\n",
            )
            .unwrap();
        }
    }
    fs::write(
        root.join("dep/tsconfig.json"),
        serde_json::to_string_pretty(&dep).unwrap() + "\n",
    )
    .unwrap();
    fs::write(root.join("dep/main.ts"), "export const dependency = 1;\n").unwrap();
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_string_pretty(&config).unwrap() + "\n",
    )
    .unwrap();
    if case == "repointed-symlink" {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            fs::create_dir_all(root.join("metadata")).unwrap();
            for name in ["a", "b"] {
                fs::write(
                    root.join(format!("metadata/{name}.json")),
                    "{\"type\":\"commonjs\"}\n",
                )
                .unwrap();
            }
            symlink("metadata/a.json", root.join("package.json")).unwrap();
        }
    } else {
        fs::write(root.join("package.json"), "{\"type\":\"commonjs\"}\n").unwrap();
    }
    let source = root.join(file);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(
        source,
        if case == "paths-types" {
            "import type { Marker } from 'alias';\nexport const value: Marker = { id: 1 };\n"
        } else {
            "export const value = 1;\n"
        },
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

fn rebuilt(output: &Output, root: &Path) -> Vec<String> {
    report(output)
        .lines()
        .filter_map(|line| {
            line.strip_prefix("Building project: ").or_else(|| {
                line.split("Building project '")
                    .nth(1)
                    .and_then(|rest| rest.split('\'').next())
            })
        })
        .map(|project| {
            Path::new(project)
                .strip_prefix(root)
                .unwrap_or_else(|_| Path::new(project))
                .to_string_lossy()
                .trim_start_matches("./")
                .replace('\\', "/")
        })
        .collect()
}

fn exercise(executable: &Path, label: &str, case: &str) -> Vec<Vec<String>> {
    let root = temporary(label);
    materialize(&root, case);
    let cold = build(executable, &root);
    assert!(cold.status.success(), "{}", report(&cold));
    let cold_built = rebuilt(&cold, &root);
    let script = root.join(if case == "nearer-package" {
        "src/nested/main.js"
    } else {
        "src/main.js"
    });
    let original_script = fs::read_to_string(&script).unwrap();
    let mut rows = vec![cold_built];
    let changes = if case == "paths-types" {
        vec!["paths", "types"]
    } else {
        vec![case]
    };
    for change in changes {
        mutate(&root, change);
        let changed = build(executable, &root);
        assert!(changed.status.success(), "{change}: {}", report(&changed));
        rows.push(rebuilt(&changed, &root));
        if matches!(case, "manifest" | "nearer-package")
            && executable == Path::new(env!("CARGO_BIN_EXE_bluetsc"))
        {
            let current_script = fs::read_to_string(&script).unwrap();
            assert_ne!(
                original_script, current_script,
                "{change} must change module-kind emission"
            );
            assert!(
                current_script.contains("export var value"),
                "{current_script}"
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
    rows
}

fn mutate(root: &Path, change: &str) {
    match change {
        "manifest" => fs::write(root.join("package.json"), "{\"type\":\"module\"}\n").unwrap(),
        "nearer-package" => fs::write(
            root.join("src/nested/package.json"),
            "{\"type\":\"module\"}\n",
        )
        .unwrap(),
        "repointed-symlink" => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::symlink;
                fs::remove_file(root.join("package.json")).unwrap();
                symlink("metadata/b.json", root.join("package.json")).unwrap();
            }
        }
        "paths" | "types" => {
            let config_path = root.join("tsconfig.json");
            let mut config: serde_json::Value =
                serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
            if change == "paths" {
                config["compilerOptions"]["paths"] = json!({"alias": ["types/two.d.ts"]});
            } else {
                config["compilerOptions"]["types"] = json!(["two"]);
            }
            fs::write(
                config_path,
                serde_json::to_string_pretty(&config).unwrap() + "\n",
            )
            .unwrap();
        }
        "unrelated" => fs::write(root.join("notes.txt"), "not an input\n").unwrap(),
        other => panic!("unknown invalidation case {other}"),
    }
}

fn matrix() -> Vec<(String, Vec<Vec<String>>)> {
    MATRIX
        .lines()
        .map(|line| {
            let (case, rows) = line.split_once('\t').expect("case and rows");
            let rows = rows
                .split('|')
                .map(|row| {
                    if row == "-" {
                        Vec::new()
                    } else {
                        row.split(';').map(str::to_string).collect()
                    }
                })
                .collect();
            (case.to_string(), rows)
        })
        .collect()
}

fn matrix_row(case: &str, rows: &[Vec<String>]) -> String {
    format!(
        "{case}\t{}\n",
        rows.iter()
            .map(|row| {
                if row.is_empty() {
                    "-".to_string()
                } else {
                    row.join(";")
                }
            })
            .collect::<Vec<_>>()
            .join("|")
    )
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn pinned_rebuild_matrix() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let observed = CASES
        .iter()
        .filter(|case| cfg!(unix) || **case != "repointed-symlink")
        .map(|case| {
            (
                (*case).to_string(),
                exercise(&tsc, &format!("oracle-{case}"), case),
            )
        })
        .collect::<Vec<_>>();
    if env::var("BLUEICE_WRITE_BUILD_INVALIDATION_MATRIX").as_deref() == Ok("1") {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_build_invalidation/build-invalidation-matrix.tsv");
        fs::write(
            path,
            observed
                .iter()
                .map(|(case, rows)| matrix_row(case, rows))
                .collect::<String>(),
        )
        .unwrap();
    } else {
        let recorded = matrix();
        assert_eq!(
            observed,
            recorded
                .into_iter()
                .filter(|(case, _)| cfg!(unix) || case != "repointed-symlink")
                .collect::<Vec<_>>()
        );
    }
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3"]
fn pinned_scope_edits_leave_stale_output_without_force() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    for case in ["manifest", "nearer-package"] {
        let root = temporary(&format!("forced-{case}"));
        materialize(&root, case);
        let cold = build(&tsc, &root);
        assert!(cold.status.success(), "{}", report(&cold));
        let script = root.join(if case == "nearer-package" {
            "src/nested/main.js"
        } else {
            "src/main.js"
        });
        let original = fs::read(&script).unwrap();
        mutate(&root, case);
        let stale = build(&tsc, &root);
        assert!(stale.status.success(), "{}", report(&stale));
        assert!(
            rebuilt(&stale, &root).is_empty(),
            "{case}: {}",
            report(&stale)
        );
        assert_eq!(fs::read(&script).unwrap(), original, "{case}");
        let forced = Command::new(&tsc)
            .current_dir(&root)
            .args(["--build", "tsconfig.json", "--force", "--pretty", "false"])
            .output()
            .unwrap();
        assert!(forced.status.success(), "{case}: {}", report(&forced));
        assert_ne!(fs::read(&script).unwrap(), original, "{case}");
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn matrix_has_all_five_unique_cases() {
    let recorded = matrix();
    assert_eq!(recorded.len(), CASES.len());
    assert_eq!(
        recorded
            .iter()
            .map(|(case, _)| case.as_str())
            .collect::<Vec<_>>(),
        CASES
    );
    assert_eq!(
        recorded
            .iter()
            .map(|(case, _)| case)
            .collect::<BTreeSet<_>>()
            .len(),
        CASES.len()
    );
}

fn assert_public(case: &str) {
    let observed = exercise(
        Path::new(env!("CARGO_BIN_EXE_bluetsc")),
        &format!("bluetsc-{case}"),
        case,
    );
    let expected_change = if case == "unrelated" {
        Vec::new()
    } else {
        vec!["tsconfig.json".to_string()]
    };
    assert_eq!(
        observed[0],
        vec!["dep/tsconfig.json", "tsconfig.json"],
        "{case}: {observed:?}"
    );
    assert!(
        observed.iter().skip(1).all(|row| row == &expected_change),
        "{case}: {observed:?}"
    );
}

#[test]
fn changed_manifest_invalidates() {
    assert_public("manifest");
}

#[test]
fn nearer_package_invalidates() {
    assert_public("nearer-package");
}

#[cfg(unix)]
#[test]
fn repointed_symlink_invalidates_even_with_identical_bytes() {
    assert_public("repointed-symlink");
}

#[test]
fn changed_paths_and_types_each_invalidate() {
    assert_public("paths-types");
}

#[test]
fn unrelated_file_does_not_invalidate() {
    assert_public("unrelated");
}
