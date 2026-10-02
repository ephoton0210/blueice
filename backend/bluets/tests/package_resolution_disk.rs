// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The package resolver on a real disk (J.4.3): canonical roots, symlinks that
//! stay inside or leave them, and cache invalidation by revalidation.

use std::fs;
use std::path::{Path, PathBuf};

use blueice_bluets::package_resolution::{
    ImportMode, ModuleResolution, OsPackageFs, PackageResolver, PackageResolverConfig, ResolveError,
};

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Self {
        let path = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("bluets-pkgdisk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn resolver(&self, roots: &[&str]) -> PackageResolver<OsPackageFs> {
        PackageResolver::new(
            OsPackageFs,
            PackageResolverConfig {
                roots: roots.iter().map(|root| self.0.join(root)).collect(),
                resolution: ModuleResolution::Node16,
                custom_conditions: Vec::new(),
            },
        )
    }

    fn at(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn link(target: &Path, at: &Path) {
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    let _ = fs::remove_file(at);
    std::os::unix::fs::symlink(target, at).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinks_inside_the_roots_are_followed_and_those_that_leave_are_not() {
    let tree = Tree::new("links");
    tree.write("app/.store/inside/index.d.ts", "");
    tree.write("elsewhere/outside/index.d.ts", "");
    link(
        &tree.at("app/.store/inside"),
        &tree.at("app/node_modules/inside"),
    );
    link(
        &tree.at("elsewhere/outside"),
        &tree.at("app/node_modules/outside"),
    );
    let resolver = tree.resolver(&["app"]);
    let from = tree.at("app/src");
    fs::create_dir_all(&from).unwrap();

    let inside = resolver
        .resolve(&from, "inside", ImportMode::Import)
        .unwrap();
    assert_eq!(inside.path, tree.at("app/.store/inside/index.d.ts"));
    let error = resolver
        .resolve(&from, "outside", ImportMode::Import)
        .unwrap_err();
    assert!(
        matches!(error, ResolveError::OutsideRoots { .. }),
        "{error}"
    );
    // Authorizing the other tree is the owner's explicit act.
    let widened = tree.resolver(&["app", "elsewhere"]);
    assert!(widened
        .resolve(&from, "outside", ImportMode::Import)
        .is_ok());
}

#[cfg(unix)]
#[test]
fn a_symlinked_file_inside_a_package_that_leaves_the_roots_is_refused() {
    let tree = Tree::new("filelink");
    tree.write(
        "app/node_modules/a/package.json",
        r#"{"types":"leak.d.ts"}"#,
    );
    tree.write("secret/leak.d.ts", "");
    link(
        &tree.at("secret/leak.d.ts"),
        &tree.at("app/node_modules/a/leak.d.ts"),
    );
    let from = tree.at("app/src");
    fs::create_dir_all(&from).unwrap();
    let error = tree
        .resolver(&["app"])
        .resolve(&from, "a", ImportMode::Import)
        .unwrap_err();
    assert!(
        matches!(error, ResolveError::OutsideRoots { .. }),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn a_node_modules_directory_that_is_itself_a_link_out_is_refused_per_package() {
    let tree = Tree::new("modules-link");
    tree.write("shared/node_modules/a/index.d.ts", "");
    fs::create_dir_all(tree.at("app")).unwrap();
    link(
        &tree.at("shared/node_modules"),
        &tree.at("app/node_modules"),
    );
    let from = tree.at("app/src");
    fs::create_dir_all(&from).unwrap();
    let error = tree
        .resolver(&["app"])
        .resolve(&from, "a", ImportMode::Import)
        .unwrap_err();
    assert!(
        matches!(error, ResolveError::OutsideRoots { .. }),
        "{error}"
    );
}

#[test]
fn path_shaped_specifiers_and_traversals_are_never_looked_up() {
    let tree = Tree::new("traversal");
    tree.write("app/node_modules/a/index.d.ts", "");
    tree.write("secret.d.ts", "");
    let from = tree.at("app/src");
    fs::create_dir_all(&from).unwrap();
    let resolver = tree.resolver(&["app"]);
    for specifier in [
        "../secret",
        "a/../../../secret",
        "/etc/passwd",
        "a/./index",
        "a\\..\\..\\secret",
        "file:///etc/passwd",
    ] {
        assert!(
            matches!(
                resolver.resolve(&from, specifier, ImportMode::Import),
                Err(ResolveError::InvalidSpecifier(_))
            ),
            "{specifier}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_cached_resolution_is_discarded_when_what_it_depended_on_changes() {
    let tree = Tree::new("invalidate");
    tree.write("app/node_modules/a/package.json", r#"{"types":"a.d.ts"}"#);
    tree.write("app/node_modules/a/a.d.ts", "");
    let from = tree.at("app/src");
    fs::create_dir_all(&from).unwrap();
    let resolver = tree.resolver(&["app"]);
    resolver.resolve(&from, "a", ImportMode::Import).unwrap();
    let before = resolver.fingerprint();
    assert!(resolver.revalidate());

    // A nearer installation shadows it.
    tree.write("app/src/node_modules/a/index.d.ts", "");
    assert!(!resolver.revalidate());
    fs::remove_dir_all(tree.at("app/src/node_modules")).unwrap();
    assert!(resolver.revalidate());

    // The manifest is edited.
    tree.write(
        "app/node_modules/a/package.json",
        r#"{"types":"a.d.ts","version":"9"}"#,
    );
    assert!(!resolver.revalidate());
    tree.write("app/node_modules/a/package.json", r#"{"types":"a.d.ts"}"#);
    assert!(resolver.revalidate());
    assert_eq!(resolver.fingerprint(), before);

    // A symlinked package is repointed.
    tree.write("app/.store/one/index.d.ts", "");
    tree.write("app/.store/two/index.d.ts", "");
    link(&tree.at("app/.store/one"), &tree.at("app/node_modules/b"));
    let linked = tree.resolver(&["app"]);
    linked.resolve(&from, "b", ImportMode::Import).unwrap();
    assert!(linked.revalidate());
    link(&tree.at("app/.store/two"), &tree.at("app/node_modules/b"));
    assert!(!linked.revalidate());
}
