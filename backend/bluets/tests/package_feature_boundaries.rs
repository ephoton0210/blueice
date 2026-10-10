// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! New resolution inputs preserve canonical read bounds and cache identity.

use blueice_bluets::package_resolution::{
    ImportMode, ModuleResolution, OsPackageFs, PackageFs, PackageResolver, PackageResolverConfig,
    ResolveError,
};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::{env, fs};

#[derive(Clone)]
struct RecordedFs(Arc<Mutex<Vec<PathBuf>>>);

impl PackageFs for RecordedFs {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.0.lock().unwrap().push(path.to_path_buf());
        OsPackageFs.read_to_string(path)
    }
    fn is_file(&self, path: &Path) -> bool {
        OsPackageFs.is_file(path)
    }
    fn is_dir(&self, path: &Path) -> bool {
        OsPackageFs.is_dir(path)
    }
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        OsPackageFs.canonicalize(path)
    }
}

struct Tree {
    root: PathBuf,
    reads: Arc<Mutex<Vec<PathBuf>>>,
}

impl Tree {
    fn new(label: &str) -> Self {
        let root = fs::canonicalize(env::temp_dir()).unwrap().join(format!(
            "bluets-package-feature-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app/src")).unwrap();
        Self {
            root,
            reads: Arc::default(),
        }
    }

    fn write(&self, name: &str, contents: &str) {
        let path = self.root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn resolver(&self, resolution: ModuleResolution) -> PackageResolver<RecordedFs> {
        PackageResolver::new(
            RecordedFs(self.reads.clone()),
            PackageResolverConfig {
                roots: vec![self.root.join("app")],
                resolution,
                custom_conditions: Vec::new(),
            },
        )
    }

    fn resolve(&self, resolver: &PackageResolver<RecordedFs>) -> Result<PathBuf, ResolveError> {
        resolver
            .resolve(&self.root.join("app/src"), "pkg", ImportMode::Require)
            .map(|file| file.path)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn types_version_target_escape_refuses_before_reading_outside_roots() {
    let tree = Tree::new("escape");
    tree.write(
        "app/node_modules/pkg/package.json",
        r#"{"types":"index.d.ts","typesVersions":{">=5":{"*":["../../../outside.d.ts"]}}}"#,
    );
    tree.write("outside.d.ts", "export type Answer = number;\n");
    let error = tree
        .resolve(&tree.resolver(ModuleResolution::Node10))
        .unwrap_err();
    assert!(matches!(error, ResolveError::InvalidPackageJson { .. }));
    assert!(tree
        .reads
        .lock()
        .unwrap()
        .iter()
        .all(|path| path.starts_with(tree.root.join("app"))));
}

#[test]
#[cfg(unix)]
fn package_manifest_symlink_refuses_before_reading_outside_roots() {
    let tree = Tree::new("manifest-link");
    tree.write(
        "app/node_modules/pkg/index.d.ts",
        "export type Answer = number;\n",
    );
    tree.write("outside.json", r#"{"types":"index.d.ts"}"#);
    std::os::unix::fs::symlink(
        tree.root.join("outside.json"),
        tree.root.join("app/node_modules/pkg/package.json"),
    )
    .unwrap();
    let error = tree
        .resolve(&tree.resolver(ModuleResolution::Node10))
        .unwrap_err();
    assert!(matches!(error, ResolveError::OutsideRoots { .. }));
    assert!(tree.reads.lock().unwrap().is_empty());
}

#[test]
fn changing_types_versions_invalidates_the_observed_manifest_and_target() {
    let tree = Tree::new("versions");
    tree.write(
        "app/node_modules/pkg/package.json",
        r#"{"types":"index.d.ts","typesVersions":{">=5":{"*":["current/*"]}}}"#,
    );
    tree.write(
        "app/node_modules/pkg/index.d.ts",
        "export type Answer = string;\n",
    );
    tree.write(
        "app/node_modules/pkg/current/index.d.ts",
        "export type Answer = number;\n",
    );
    let resolver = tree.resolver(ModuleResolution::Node10);
    assert_eq!(
        tree.resolve(&resolver).unwrap(),
        tree.root.join("app/node_modules/pkg/current/index.d.ts")
    );
    let before = resolver.fingerprint();
    assert!(resolver.revalidate());
    tree.write(
        "app/node_modules/pkg/package.json",
        r#"{"types":"index.d.ts","typesVersions":{">=6":{"*":["current/*"]}}}"#,
    );
    assert!(!resolver.revalidate());
    let changed = tree.resolver(ModuleResolution::Node10);
    assert_eq!(
        tree.resolve(&changed).unwrap(),
        tree.root.join("app/node_modules/pkg/index.d.ts")
    );
    assert_ne!(changed.fingerprint(), before);
}

#[test]
fn nearer_classic_file_invalidates_the_observed_absent_candidate() {
    let tree = Tree::new("classic");
    tree.write("app/value.ts", "export type Answer = number;\n");
    let resolver = tree.resolver(ModuleResolution::Classic);
    let from = tree.root.join("app/src");
    assert_eq!(
        resolver
            .resolve(&from, "value", ImportMode::Import)
            .unwrap()
            .path,
        tree.root.join("app/value.ts")
    );
    assert!(resolver.revalidate());
    tree.write("app/src/value.ts", "export type Answer = string;\n");
    assert!(!resolver.revalidate());
    assert_eq!(
        tree.resolver(ModuleResolution::Classic)
            .resolve(&from, "value", ImportMode::Import)
            .unwrap()
            .path,
        tree.root.join("app/src/value.ts")
    );
}
