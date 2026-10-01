// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Namespace declaration output against pinned TypeScript (J.3.5.4): the
//! `.d.ts` BlueTSC prints for a module of namespaces must be accepted by `tsc`,
//! and a consumer must be accepted or rejected the same way whether it reads
//! the original source or that declaration file.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

const SOURCE: &str =
    include_str!("fixtures/typescript_oracle/namespace-declaration-output/main.ts");

/// `(consumer text, pinned TypeScript accepts it)`.
const CONSUMERS: &[(&str, bool)] = &[
    (
        "import { N, A, Types, Local, Plain } from './main';\n\
         const a: number = N.a;\n\
         const i: N.I = { q: 1 };\n\
         const c: N.C = new N.C();\n\
         const e: N.E = N.E.Y;\n\
         const z: number = N.Inner.z;\n\
         const j: N.Inner.J = { q: 2 };\n\
         const k: string = A.B.C.k;\n\
         const l: number = Local.l;\n\
         const p: Types.P = { x: 1 };\n\
         const o: string = Plain.only;\n\
         const u: { h: number } = N.usesHidden();\n\
         const t: N.T = 'v';\n\
         export { a, i, c, e, z, j, k, l, p, o, u, t };\n",
        true,
    ),
    (
        "import { N } from './main';\nconst s: string = N.a;\nexport { s };\n",
        false,
    ),
    (
        "import { N } from './main';\nconst h: N.Hidden = { h: 1 };\nexport { h };\n",
        false,
    ),
    (
        "import { N } from './main';\nconst r: number = N.f('x');\nexport { r };\n",
        false,
    ),
    (
        "import { Types } from './main';\nconst p: Types.P = { y: 1 };\nexport { p };\n",
        false,
    ),
];

#[test]
#[ignore = "requires BLUEICE_BLUETSC_ORACLE to point to the pinned TypeScript compiler"]
fn the_declaration_bluetsc_prints_for_namespaces_is_valid_and_means_the_same_to_a_consumer() {
    let tsc = env::var_os("BLUEICE_BLUETSC_ORACLE")
        .map(PathBuf::from)
        .expect("set BLUEICE_BLUETSC_ORACLE to the TypeScript 5.9.3 tsc executable");
    let version = Command::new(&tsc).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).contains("5.9.3"));

    let compiled = compile(
        "memory:///main.ts",
        &MapLoader::from([ModuleSource::new("memory:///main.ts", SOURCE)]),
        CompilerOptions {
            declaration: true,
            ..CompilerOptions::default()
        },
    );
    assert!(
        compiled.output.is_some(),
        "BlueTSC rejected the module: {:#?}",
        compiled.diagnostics
    );
    let declaration = compiled.output.as_ref().unwrap().artifacts["memory:///main.ts"]
        .declaration
        .clone()
        .expect("a declaration was requested");

    let from_source = Directory::new();
    let from_declaration = Directory::new();
    fs::write(from_source.path().join("main.ts"), SOURCE).unwrap();
    fs::write(from_declaration.path().join("main.d.ts"), &declaration).unwrap();
    // The declaration alone must be valid.
    assert!(
        check(&tsc, from_declaration.path(), &["main.d.ts"]),
        "tsc rejected the declaration:\n{declaration}"
    );
    for (index, (consumer, accepts)) in CONSUMERS.iter().enumerate() {
        for directory in [&from_source, &from_declaration] {
            fs::write(directory.path().join("consumer.ts"), consumer).unwrap();
            assert_eq!(
                check(&tsc, directory.path(), &["consumer.ts"]),
                *accepts,
                "consumer {index} against {}:\n{consumer}\n--- declaration ---\n{declaration}",
                if directory.path() == from_source.path() {
                    "the source"
                } else {
                    "the declaration"
                }
            );
        }
    }
}

fn check(tsc: &Path, directory: &Path, files: &[&str]) -> bool {
    Command::new(tsc)
        .current_dir(directory)
        .args([
            "--target",
            "ES2022",
            "--module",
            "ES2022",
            "--strict",
            "--noEmit",
            "--pretty",
            "false",
            "--allowImportingTsExtensions",
        ])
        .args(files)
        .output()
        .unwrap()
        .status
        .success()
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "blueice-bluets-namespace-declaration-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
