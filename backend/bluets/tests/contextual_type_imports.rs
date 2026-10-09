// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Contextual `type` binding names keep value and erased-import meanings.

use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

fn report(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn declarations(root: &Path) -> BTreeMap<String, String> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".d.ts"))
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
            )
        })
        .collect()
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn contextual_type_default_and_equals_bindings_match_pinned_execution_and_declarations() {
    let tsc = PathBuf::from(env::var_os("BLUEICE_BLUETSC_ORACLE").unwrap());
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(version.status.success(), "{}", report(&version));
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "Version 5.9.3"
    );
    let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
        "bluets-contextual-type-imports-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let mut references = Vec::new();
    for (name,module,main,dependency,source) in [
        ("default-esm","esnext","import type from './dep.js'; console.log(type);","dep.ts","export default 42;"),
        ("default-commonjs","commonjs","import type from './dep'; console.log(type);","dep.ts","export default 42;"),
        ("equals-value","commonjs","import type = require('./dep'); console.log(type.answer);","dep.ts","export const answer: number = 42;"),
        ("equals-erased","commonjs","import type type = require('./dep'); const item: type = {value:42}; console.log(item.value);","dep.d.ts","interface Item {value:number;} export = Item;"),
    ] {
        let directory=root.join(name);fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("main.ts"),main).unwrap();
        fs::write(directory.join(dependency),source).unwrap();
        fs::write(directory.join("package.json"),json!({"type":if module=="commonjs" {"commonjs"}else{"module"}}).to_string()).unwrap();
        let config=directory.join("tsconfig.json");let blue=directory.join("blue");let reference=directory.join("reference");
        fs::write(&config,json!({"compilerOptions":{"target":"ES2022","module":module,"moduleResolution":"node10","strict":true,"declaration":true,"outDir":blue},"files":["main.ts"]}).to_string()).unwrap();
        let built=Command::new(&tsc).arg("--project").arg(&config).arg("--outDir").arg(&reference).output().unwrap();
        assert!(built.status.success(),"{name}: {}",report(&built));
        let run=Command::new("node").arg(reference.join("main.js")).output().unwrap();
        assert!(run.status.success(),"{name}: {}",report(&run));
        assert_eq!(run.stdout,b"42\n","{name}");
        references.push((name,config,blue,run.stdout,declarations(&reference)));
    }
    let mut failures = Vec::new();
    for (name, config, blue, stdout, expected) in references {
        let built = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .arg("--project")
            .arg(config)
            .output()
            .unwrap();
        if !built.status.success() {
            failures.push(format!("{name}: {}", report(&built)));
            continue;
        }
        let run = Command::new("node")
            .arg(blue.join("main.js"))
            .output()
            .unwrap();
        if !run.status.success() || run.stdout != stdout {
            failures.push(format!("{name}: execution differs: {}", report(&run)));
        }
        let actual = declarations(&blue);
        if actual != expected {
            failures.push(format!(
                "{name}: declarations differ: {actual:#?} != {expected:#?}"
            ));
        }
    }
    fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
