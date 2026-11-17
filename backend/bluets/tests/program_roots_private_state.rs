// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Local CLI bookkeeping stays outside Native artifacts and cannot expand output roots.

use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Project {
    base: PathBuf,
    root: PathBuf,
    state: PathBuf,
}
impl Project {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "bluets-private-state-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&base).unwrap();
        let base = fs::canonicalize(base).unwrap();
        let root = base.join("project");
        fs::create_dir(&root).unwrap();
        let result = Self {
            state: base.join("state"),
            base,
            root,
        };
        fs::write(
            result.root.join("main.ts"),
            "export const value: number = 1;\n",
        )
        .unwrap();
        fs::write(
            result.root.join("removed.ts"),
            "export const extra: number = 2;\n",
        )
        .unwrap();
        result.configure(true);
        result
    }
    fn configure(&self, removed: bool) {
        fs::write(
            self.root.join("tsconfig.json"),
            serde_json::to_vec_pretty(&json!({
                "compilerOptions":{"strict":true,"target":"ES5","module":"CommonJS",
                    "declaration":true,"sourceMap":true,"declarationMap":true,"outDir":"dist"},
                "files":if removed {vec!["main.ts","removed.ts"]} else {vec!["main.ts"]}
            }))
            .unwrap(),
        )
        .unwrap();
    }
    fn run_at(&self, state: &Path, flags: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(["--project", "tsconfig.json", "--diagnostics-json"])
            .args(flags)
            .env("BLUETSC_OWNER_STATE_DIR", state)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }
    fn run(&self) -> Output {
        self.run_at(&self.state, &[])
    }
    fn build(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args([
                "--build",
                "tsconfig.json",
                "--verbose",
                "--diagnostics-json",
            ])
            .env("BLUETSC_OWNER_STATE_DIR", &self.state)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }
    fn cold(&self) {
        let o = self.run();
        assert!(
            o.status.success(),
            "{}; retained {}",
            String::from_utf8_lossy(&o.stderr),
            self.base.display()
        );
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.base).unwrap();
        }
    }
}

fn inventory(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        if !dir.exists() {
            return;
        }
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(root, &path, files);
            } else if entry.file_type().unwrap().is_file() {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    files
}
fn receipt(project: &Project) -> PathBuf {
    let files = inventory(&project.state);
    assert_eq!(
        files.len(),
        1,
        "one bounded successful generation; retained {}",
        project.base.display()
    );
    project.state.join(files.keys().next().unwrap())
}

#[test]
fn native_artifacts_have_no_bookkeeping_file_and_private_ownership_retires_removed_roots() {
    let project = Project::new();
    project.cold();
    assert_eq!(
        fs::read_dir(&project.root).unwrap().count(),
        4,
        "only sources, config and dist"
    );
    assert!(receipt(&project).is_file());
    fs::write(project.root.join("dist/user-owned.js"), "// keep me\n").unwrap();
    project.configure(false);
    project.cold();
    for ext in ["js", "js.map", "d.ts", "d.ts.map"] {
        assert!(!project.root.join(format!("dist/removed.{ext}")).exists());
    }
    assert_eq!(
        fs::read_to_string(project.root.join("dist/user-owned.js")).unwrap(),
        "// keep me\n"
    );
}

#[test]
fn a_private_state_root_file_collision_refuses_before_changing_public_outputs() {
    let project = Project::new();
    project.cold();
    let before = inventory(&project.root);
    let previous = inventory(&project.state);
    let collision = project.base.join("collision");
    fs::write(&collision, "// outside sentinel\n").unwrap();
    let output = project.run_at(&collision, &[]);
    assert!(!output.status.success());
    assert_eq!(inventory(&project.root), before);
    assert_eq!(inventory(&project.state), previous);
    assert_eq!(
        fs::read_to_string(collision).unwrap(),
        "// outside sentinel\n"
    );
}

#[cfg(unix)]
#[test]
fn a_private_state_symlink_never_reaches_an_outside_directory() {
    let project = Project::new();
    project.cold();
    let before = inventory(&project.root);
    let outside = project.base.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), "sentinel").unwrap();
    let outside_before = inventory(&outside);
    let link = project.base.join("state-link");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    assert!(!project.run_at(&link, &[]).status.success());
    assert_eq!(inventory(&outside), outside_before);
    assert_eq!(inventory(&project.root), before);
}

#[test]
fn tampered_retirement_keeps_the_last_private_and_public_generations() {
    let project = Project::new();
    project.cold();
    let _ = receipt(&project);
    project.configure(false);
    fs::write(
        project.root.join("dist/removed.js"),
        "// externally replaced\n",
    )
    .unwrap();
    let before = inventory(&project.root);
    let state = inventory(&project.state);
    assert!(!project.run().status.success());
    assert_eq!(inventory(&project.root), before);
    assert_eq!(inventory(&project.state), state);
}

#[test]
fn malformed_and_oversized_private_receipts_refuse_before_publishing() {
    for bytes in [b"not json".to_vec(), vec![b'x'; 8 * 1024 * 1024 + 1]] {
        let project = Project::new();
        project.cold();
        let path = receipt(&project);
        fs::write(&path, bytes).unwrap();
        project.configure(false);
        let before = inventory(&project.root);
        let state = inventory(&project.state);
        assert!(!project.run().status.success());
        assert_eq!(inventory(&project.root), before);
        assert_eq!(inventory(&project.state), state);
    }
}

#[test]
fn static_checks_need_no_private_state_write_authority() {
    let project = Project::new();
    let collision = project.base.join("collision");
    fs::write(&collision, "sentinel").unwrap();
    let before = inventory(&project.root);
    assert!(project
        .run_at(&collision, &["--noEmit", "true"])
        .status
        .success());
    assert_eq!(inventory(&project.root), before);
    assert_eq!(fs::read_to_string(collision).unwrap(), "sentinel");
}

#[test]
fn a_state_root_inside_the_project_is_refused_before_it_adds_an_artifact() {
    let project = Project::new();
    let before = inventory(&project.root);
    assert!(!project
        .run_at(&project.root.join("state"), &[])
        .status
        .success());
    assert_eq!(inventory(&project.root), before);
}

#[test]
fn shared_private_storage_keeps_project_identities_separate_and_rejects_a_foreign_receipt() {
    let first = Project::new();
    first.cold();
    let first_path = receipt(&first);
    let first_bytes = fs::read(&first_path).unwrap();
    let second = Project::new();
    let output = second.run_at(&first.state, &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files = inventory(&first.state);
    assert_eq!(files.len(), 2);
    let second_path = files
        .keys()
        .map(|path| first.state.join(path))
        .find(|path| *path != first_path)
        .unwrap();
    fs::write(&second_path, &first_bytes).unwrap();
    second.configure(false);
    let before = inventory(&second.root);
    let cache = inventory(&first.state);
    assert!(!second.run_at(&first.state, &[]).status.success());
    assert_eq!(inventory(&second.root), before);
    assert_eq!(inventory(&first.state), cache);
    assert_eq!(fs::read(first_path).unwrap(), first_bytes);
}

fn legacy_receipt(project: &Project) -> PathBuf {
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(receipt(project)).unwrap()).unwrap();
    let path = project.root.join("tsconfig.bluetsc.outputs.json");
    fs::write(&path, serde_json::to_vec_pretty(&value["outputs"]).unwrap()).unwrap();
    path
}

#[test]
fn a_legacy_project_receipt_migrates_while_retirement_preserves_unrelated_files() {
    let project = Project::new();
    project.cold();
    let old = legacy_receipt(&project);
    fs::remove_file(receipt(&project)).unwrap();
    project.configure(false);
    fs::write(project.root.join("dist/user-owned.js"), "sentinel").unwrap();
    project.cold();
    assert!(!old.exists());
    assert!(receipt(&project).is_file());
    for ext in ["js", "js.map", "d.ts", "d.ts.map"] {
        assert!(!project.root.join(format!("dist/removed.{ext}")).exists());
    }
    assert_eq!(
        fs::read_to_string(project.root.join("dist/user-owned.js")).unwrap(),
        "sentinel"
    );
}

fn check_old_cache_rebuild(incremental: bool) {
    let project = Project::new();
    if incremental {
        let path = project.root.join("tsconfig.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["compilerOptions"]["incremental"] = json!(true);
        fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    }
    let cold = project.build();
    assert!(
        cold.status.success(),
        "{}",
        String::from_utf8_lossy(&cold.stderr)
    );
    let before = inventory(&project.root.join("dist"));
    let warm = project.build();
    assert!(warm.status.success());
    assert!(!String::from_utf8_lossy(&warm.stdout).contains("Building project:"));
    let state_path = project.root.join("tsconfig.bluetsc.tsbuildinfo");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["version"] = json!(73);
    fs::write(&state_path, serde_json::to_vec(&state).unwrap()).unwrap();
    let old_receipt = legacy_receipt(&project);
    let rebuilt = project.build();
    assert!(
        rebuilt.status.success(),
        "{}",
        String::from_utf8_lossy(&rebuilt.stderr)
    );
    assert!(
        String::from_utf8_lossy(&rebuilt.stdout).contains("Building project:"),
        "an old producer cache cannot bypass output migration"
    );
    assert!(!old_receipt.exists());
    let state: serde_json::Value = serde_json::from_slice(&fs::read(state_path).unwrap()).unwrap();
    assert!(state["version"].as_u64().unwrap() > 73);
    assert_eq!(inventory(&project.root.join("dist")), before);
    let old_receipt = legacy_receipt(&project);
    let current_cache = project.build();
    assert!(current_cache.status.success());
    assert!(String::from_utf8_lossy(&current_cache.stdout).contains("Building project:"));
    assert!(!old_receipt.exists());
}

#[test]
fn an_old_cache_rechecks_and_migrates_publication_metadata() {
    check_old_cache_rebuild(false);
}

#[test]
fn an_old_cache_cannot_reimport_its_public_native_state_to_bypass_migration() {
    check_old_cache_rebuild(true);
}
