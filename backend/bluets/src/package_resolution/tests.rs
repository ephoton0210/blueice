use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::*;

#[test]
fn node_scope_revalidates_absent_and_changed_nearest_manifests() {
    let fs = MemoryFs::with(&[("/project/package.json", r#"{"type":"module"}"#)]);
    let resolver = resolver(&fs, ModuleResolution::Node16);
    assert!(resolver
        .is_es_module_scope(Path::new("/project/src"))
        .unwrap());
    assert!(resolver.revalidate());
    let first = resolver.fingerprint();
    fs.write("/project/src/package.json", r#"{"type":"commonjs"}"#);
    assert!(!resolver.revalidate());
    assert!(!resolver
        .is_es_module_scope(Path::new("/project/src"))
        .unwrap());
    assert_ne!(first, resolver.fingerprint());
    assert!(resolver.revalidate());
    fs.write("/project/src/package.json", r#"{"type":"module"}"#);
    assert!(!resolver.revalidate());
}

#[test]
fn node_scope_revalidates_same_byte_manifest_symlink_replacement() {
    let fs = MemoryFs::with(&[
        ("/project/package.json", r#"{"type":"module"}"#),
        ("/outside/package.json", r#"{"type":"module"}"#),
    ]);
    let resolver = resolver(&fs, ModuleResolution::Node16);
    assert!(resolver.is_es_module_scope(Path::new("/project")).unwrap());
    assert!(resolver.revalidate());
    fs.link("/project/package.json", "/outside/package.json");
    assert!(!resolver.revalidate());
    assert!(matches!(
        resolver.is_es_module_scope(Path::new("/project")),
        Err(ResolveError::OutsideRoots { .. })
    ));
}

/// An in-memory tree with symlinks, so every resolution rule runs without a disk.
#[derive(Clone, Default)]
struct MemoryFs {
    files: Rc<RefCell<BTreeMap<PathBuf, String>>>,
    links: Rc<RefCell<BTreeMap<PathBuf, PathBuf>>>,
}

impl MemoryFs {
    fn with(files: &[(&str, &str)]) -> Self {
        let fs = Self::default();
        for (path, text) in files {
            fs.files
                .borrow_mut()
                .insert(PathBuf::from(path), text.to_string());
        }
        fs
    }

    fn link(&self, from: &str, to: &str) {
        self.links
            .borrow_mut()
            .insert(PathBuf::from(from), PathBuf::from(to));
    }

    fn write(&self, path: &str, text: &str) {
        self.files
            .borrow_mut()
            .insert(PathBuf::from(path), text.to_string());
    }

    fn remove(&self, path: &str) {
        self.files.borrow_mut().remove(Path::new(path));
    }

    fn real(&self, path: &Path) -> PathBuf {
        let mut current = PathBuf::from("/");
        for component in path.components().skip(1) {
            current.push(component);
            for _ in 0..16 {
                let target = self.links.borrow().get(&current).cloned();
                match target {
                    Some(target) => current = target,
                    None => break,
                }
            }
        }
        current
    }
}

impl PackageFs for MemoryFs {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.files
            .borrow()
            .get(&self.real(path))
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
    fn is_file(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(&self.real(path))
    }
    fn is_dir(&self, path: &Path) -> bool {
        let real = self.real(path);
        self.files
            .borrow()
            .keys()
            .any(|file| file != &real && file.starts_with(&real))
            || self
                .links
                .borrow()
                .keys()
                .any(|link| link != &real && link.starts_with(&real))
    }
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        let real = self.real(path);
        if self.is_file(&real) || self.is_dir(&real) {
            Ok(real)
        } else {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    }
}

fn resolver(fs: &MemoryFs, resolution: ModuleResolution) -> PackageResolver<MemoryFs> {
    PackageResolver::new(
        fs.clone(),
        PackageResolverConfig {
            roots: vec![PathBuf::from("/project")],
            resolution,
            custom_conditions: Vec::new(),
        },
    )
}

fn resolve(
    fs: &MemoryFs,
    resolution: ModuleResolution,
    specifier: &str,
    mode: ImportMode,
) -> Result<ResolvedPackageFile, ResolveError> {
    resolver(fs, resolution).resolve(Path::new("/project/src"), specifier, mode)
}

fn path_of(result: Result<ResolvedPackageFile, ResolveError>) -> PathBuf {
    result.unwrap().path
}

#[test]
fn node10_reads_types_then_typings_then_main_then_index() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"types":"t/a.d.ts","main":"m.js"}"#,
        ),
        ("/project/node_modules/a/t/a.d.ts", ""),
        (
            "/project/node_modules/b/package.json",
            r#"{"main":"lib/main.js"}"#,
        ),
        ("/project/node_modules/b/lib/main.ts", ""),
        ("/project/node_modules/c/index.d.ts", ""),
        (
            "/project/node_modules/d/package.json",
            r#"{"typings":"./x.d.ts","types":"y.d.ts"}"#,
        ),
        ("/project/node_modules/d/x.d.ts", ""),
        ("/project/node_modules/d/y.d.ts", ""),
    ]);
    let at = |name| {
        path_of(resolve(
            &fs,
            ModuleResolution::Node10,
            name,
            ImportMode::Import,
        ))
    };
    assert_eq!(at("a"), PathBuf::from("/project/node_modules/a/t/a.d.ts"));
    assert_eq!(
        at("b"),
        PathBuf::from("/project/node_modules/b/lib/main.ts")
    );
    assert_eq!(at("c"), PathBuf::from("/project/node_modules/c/index.d.ts"));
    assert_eq!(at("d"), PathBuf::from("/project/node_modules/d/x.d.ts"));
}

#[test]
fn a_package_is_found_with_its_declaration_marker_version_and_root() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"version":"1.2.3","types":"a.d.ts"}"#,
        ),
        ("/project/node_modules/a/a.d.ts", ""),
    ]);
    let found = resolve(&fs, ModuleResolution::Node10, "a", ImportMode::Import).unwrap();
    assert!(found.declaration);
    let package = found.package.unwrap();
    assert_eq!(package.name, "a");
    assert_eq!(package.version.as_deref(), Some("1.2.3"));
    assert_eq!(package.root, PathBuf::from("/project/node_modules/a"));
    assert!(!package.from_types_package);
}

#[test]
fn subpaths_resolve_as_files_then_directories() {
    let fs = MemoryFs::with(&[
        ("/project/node_modules/a/package.json", "{}"),
        ("/project/node_modules/a/util.d.ts", ""),
        ("/project/node_modules/a/lib/index.ts", ""),
        (
            "/project/node_modules/a/sub/package.json",
            r#"{"types":"entry.d.ts"}"#,
        ),
        ("/project/node_modules/a/sub/entry.d.ts", ""),
    ]);
    let at = |name| {
        path_of(resolve(
            &fs,
            ModuleResolution::Node10,
            name,
            ImportMode::Import,
        ))
    };
    assert_eq!(
        at("a/util"),
        PathBuf::from("/project/node_modules/a/util.d.ts")
    );
    assert_eq!(
        at("a/util.js"),
        PathBuf::from("/project/node_modules/a/util.d.ts")
    );
    assert_eq!(
        at("a/lib"),
        PathBuf::from("/project/node_modules/a/lib/index.ts")
    );
    assert_eq!(
        at("a/sub"),
        PathBuf::from("/project/node_modules/a/sub/entry.d.ts")
    );
}

#[test]
fn types_packages_are_found_and_scoped_names_are_mangled() {
    let fs = MemoryFs::with(&[
        ("/project/node_modules/@types/plain/index.d.ts", ""),
        ("/project/node_modules/@types/scope__pkg/index.d.ts", ""),
    ]);
    let found = resolve(&fs, ModuleResolution::Node10, "plain", ImportMode::Import).unwrap();
    assert!(found.package.unwrap().from_types_package);
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Node10,
            "@scope/pkg",
            ImportMode::Import
        )),
        PathBuf::from("/project/node_modules/@types/scope__pkg/index.d.ts")
    );
    assert_eq!(types_package_name("@scope/pkg"), "@types/scope__pkg");
    assert_eq!(types_package_name("pkg"), "@types/pkg");
}

#[test]
fn the_package_itself_wins_over_its_types_package_in_the_same_node_modules() {
    let fs = MemoryFs::with(&[
        ("/project/node_modules/a/index.d.ts", ""),
        ("/project/node_modules/@types/a/index.d.ts", ""),
    ]);
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Node10,
            "a",
            ImportMode::Import
        )),
        PathBuf::from("/project/node_modules/a/index.d.ts")
    );
}

#[test]
fn the_nearest_node_modules_wins_and_a_farther_one_is_the_fallback() {
    let fs = MemoryFs::with(&[
        ("/project/node_modules/a/index.d.ts", "outer"),
        ("/project/src/node_modules/a/index.d.ts", "inner"),
        ("/project/node_modules/b/index.d.ts", "outer-only"),
    ]);
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Node10,
            "a",
            ImportMode::Import
        )),
        PathBuf::from("/project/src/node_modules/a/index.d.ts")
    );
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Node10,
            "b",
            ImportMode::Import
        )),
        PathBuf::from("/project/node_modules/b/index.d.ts")
    );
}

#[test]
fn the_search_stops_at_the_authorized_roots() {
    // `/node_modules` is above the only root, so it is never consulted.
    let fs = MemoryFs::with(&[("/node_modules/a/index.d.ts", "")]);
    let error = resolve(&fs, ModuleResolution::Node10, "a", ImportMode::Import).unwrap_err();
    assert!(matches!(error, ResolveError::NotFound { .. }), "{error}");
    assert!(error
        .to_string()
        .contains("nothing is installed implicitly"));
}

#[test]
fn a_second_root_extends_the_search_to_a_hoisted_tree() {
    let fs = MemoryFs::with(&[("/workspace/node_modules/a/index.d.ts", "")]);
    let resolver = PackageResolver::new(
        fs,
        PackageResolverConfig {
            roots: vec![PathBuf::from("/workspace/app"), PathBuf::from("/workspace")],
            resolution: ModuleResolution::Node10,
            custom_conditions: Vec::new(),
        },
    );
    let found = resolver
        .resolve(Path::new("/workspace/app/src"), "a", ImportMode::Import)
        .unwrap();
    assert_eq!(
        found.path,
        PathBuf::from("/workspace/node_modules/a/index.d.ts")
    );
}

#[test]
fn node10_ignores_exports() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"exports":{".":"./nope.d.ts"},"types":"real.d.ts"}"#,
        ),
        ("/project/node_modules/a/real.d.ts", ""),
    ]);
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Node10,
            "a",
            ImportMode::Import
        )),
        PathBuf::from("/project/node_modules/a/real.d.ts")
    );
}

const EXPORTS_PACKAGE: &str = r#"{
  "name": "a",
  "exports": {
    ".": {
      "types": "./types/index.d.ts",
      "import": "./esm/index.js",
      "require": "./cjs/index.js"
    },
    "./feature": { "node": "./node/feature.d.ts", "default": "./feature.d.ts" },
    "./internal/*": null,
    "./lib/*": "./dist/*.js",
    "./data/*.json": "./data/*.d.ts",
    "./list": ["./missing.d.ts", "./list.d.ts"]
  }
}"#;

fn exports_fs() -> MemoryFs {
    MemoryFs::with(&[
        ("/project/node_modules/a/package.json", EXPORTS_PACKAGE),
        ("/project/node_modules/a/types/index.d.ts", ""),
        ("/project/node_modules/a/esm/index.d.ts", ""),
        ("/project/node_modules/a/cjs/index.d.ts", ""),
        ("/project/node_modules/a/node/feature.d.ts", ""),
        ("/project/node_modules/a/feature.d.ts", ""),
        ("/project/node_modules/a/dist/x.d.ts", ""),
        ("/project/node_modules/a/dist/internal.d.ts", ""),
        ("/project/node_modules/a/data/config.d.ts", ""),
        ("/project/node_modules/a/list.d.ts", ""),
    ])
}

#[test]
fn exports_conditions_pick_types_first_and_follow_the_first_matching_key() {
    let fs = exports_fs();
    for mode in [ImportMode::Import, ImportMode::Require] {
        assert_eq!(
            path_of(resolve(&fs, ModuleResolution::Node16, "a", mode)),
            PathBuf::from("/project/node_modules/a/types/index.d.ts")
        );
    }
}

#[test]
fn the_import_mode_selects_import_or_require_when_there_is_no_types_condition() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"exports":{"import":"./m.js","require":"./c.js"}}"#,
        ),
        ("/project/node_modules/a/m.d.ts", ""),
        ("/project/node_modules/a/c.d.ts", ""),
    ]);
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Bundler,
            "a",
            ImportMode::Import
        )),
        PathBuf::from("/project/node_modules/a/m.d.ts")
    );
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Bundler,
            "a",
            ImportMode::Require
        )),
        PathBuf::from("/project/node_modules/a/c.d.ts")
    );
}

#[test]
fn node16_has_the_node_condition_and_bundler_does_not() {
    let fs = exports_fs();
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Node16,
            "a/feature",
            ImportMode::Import
        )),
        PathBuf::from("/project/node_modules/a/node/feature.d.ts")
    );
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Bundler,
            "a/feature",
            ImportMode::Import
        )),
        PathBuf::from("/project/node_modules/a/feature.d.ts")
    );
}

#[test]
fn custom_conditions_are_added() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"exports":{"development":"./dev.d.ts","default":"./prod.d.ts"}}"#,
        ),
        ("/project/node_modules/a/dev.d.ts", ""),
        ("/project/node_modules/a/prod.d.ts", ""),
    ]);
    let resolver = PackageResolver::new(
        fs,
        PackageResolverConfig {
            roots: vec![PathBuf::from("/project")],
            resolution: ModuleResolution::Bundler,
            custom_conditions: vec!["development".to_string()],
        },
    );
    let found = resolver
        .resolve(Path::new("/project/src"), "a", ImportMode::Import)
        .unwrap();
    assert_eq!(
        found.path,
        PathBuf::from("/project/node_modules/a/dev.d.ts")
    );
}

#[test]
fn exports_patterns_null_blocks_and_array_fallbacks() {
    let fs = exports_fs();
    let at = |name| resolve(&fs, ModuleResolution::Node16, name, ImportMode::Import);
    assert_eq!(
        path_of(at("a/lib/x")),
        PathBuf::from("/project/node_modules/a/dist/x.d.ts")
    );
    assert_eq!(
        path_of(at("a/data/config.json")),
        PathBuf::from("/project/node_modules/a/data/config.d.ts")
    );
    assert_eq!(
        path_of(at("a/list")),
        PathBuf::from("/project/node_modules/a/list.d.ts")
    );
    assert!(matches!(
        at("a/internal/x"),
        Err(ResolveError::ExportsNotDefined { .. })
    ));
    assert!(matches!(
        at("a/never-exported"),
        Err(ResolveError::ExportsNotDefined { .. })
    ));
}

#[test]
fn a_package_with_exports_does_not_fall_back_to_main_or_files() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"exports":{".":"./index.d.ts"},"main":"other.js"}"#,
        ),
        ("/project/node_modules/a/index.d.ts", ""),
        ("/project/node_modules/a/secret.d.ts", ""),
    ]);
    assert!(resolve(&fs, ModuleResolution::Node16, "a", ImportMode::Import).is_ok());
    assert!(matches!(
        resolve(
            &fs,
            ModuleResolution::Node16,
            "a/secret",
            ImportMode::Import
        ),
        Err(ResolveError::ExportsNotDefined { .. })
    ));
    // `node10` does not read `exports` at all, so it reaches the file.
    assert!(resolve(
        &fs,
        ModuleResolution::Node10,
        "a/secret",
        ImportMode::Import
    )
    .is_ok());
}

#[test]
fn exports_with_a_shorthand_string_and_with_escaping_targets() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"exports":"./main.d.ts"}"#,
        ),
        ("/project/node_modules/a/main.d.ts", ""),
        (
            "/project/node_modules/b/package.json",
            r#"{"exports":{".":"./../escape.d.ts"}}"#,
        ),
        ("/project/node_modules/escape.d.ts", ""),
    ]);
    assert_eq!(
        path_of(resolve(
            &fs,
            ModuleResolution::Node16,
            "a",
            ImportMode::Import
        )),
        PathBuf::from("/project/node_modules/a/main.d.ts")
    );
    assert!(matches!(
        resolve(&fs, ModuleResolution::Node16, "b", ImportMode::Import),
        Err(ResolveError::InvalidPackageJson { .. })
    ));
}

#[test]
fn package_imports_resolve_within_the_nearest_package() {
    let fs = MemoryFs::with(&[
        (
            "/project/package.json",
            r##"{"imports":{"#internal":"./src/internal.ts","#dep":"a","#cond/*":{"types":"./src/*.d.ts"}}}"##,
        ),
        ("/project/src/internal.ts", ""),
        ("/project/src/thing.d.ts", ""),
        ("/project/node_modules/a/index.d.ts", ""),
    ]);
    let at = |name| resolve(&fs, ModuleResolution::Bundler, name, ImportMode::Import);
    assert_eq!(
        path_of(at("#internal")),
        PathBuf::from("/project/src/internal.ts")
    );
    assert_eq!(
        path_of(at("#cond/thing")),
        PathBuf::from("/project/src/thing.d.ts")
    );
    assert_eq!(
        path_of(at("#dep")),
        PathBuf::from("/project/node_modules/a/index.d.ts")
    );
    assert!(matches!(
        at("#missing"),
        Err(ResolveError::ImportsNotDefined { .. })
    ));
    assert!(matches!(
        resolve(
            &fs,
            ModuleResolution::Node10,
            "#internal",
            ImportMode::Import
        ),
        Err(ResolveError::ImportsNotDefined { .. })
    ));
}

#[test]
fn javascript_only_packages_say_so_instead_of_pretending_to_be_typed() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"main":"index.js"}"#,
        ),
        ("/project/node_modules/a/index.js", ""),
    ]);
    let error = resolve(&fs, ModuleResolution::Node10, "a", ImportMode::Import).unwrap_err();
    assert!(
        matches!(error, ResolveError::JavaScriptOnly { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("only to JavaScript"));
}

#[test]
fn a_missing_package_is_not_found_and_invalid_specifiers_are_refused() {
    let fs = MemoryFs::with(&[("/project/node_modules/a/index.d.ts", "")]);
    assert!(matches!(
        resolve(&fs, ModuleResolution::Node10, "missing", ImportMode::Import),
        Err(ResolveError::NotFound { .. })
    ));
    for bad in [
        "",
        ".",
        "..",
        "a//b",
        "a/../b",
        "a\\b",
        "@scope",
        "/abs",
        "http://x/y",
        "./rel",
    ] {
        assert!(
            matches!(
                resolve(&fs, ModuleResolution::Node10, bad, ImportMode::Import),
                Err(ResolveError::InvalidSpecifier(_))
            ),
            "{bad}"
        );
    }
}

#[test]
fn specifiers_split_into_name_and_subpath() {
    assert_eq!(split_package_specifier("a"), Some(("a", String::new())));
    assert_eq!(
        split_package_specifier("a/b/c"),
        Some(("a", "/b/c".to_string()))
    );
    assert_eq!(
        split_package_specifier("@s/p"),
        Some(("@s/p", String::new()))
    );
    assert_eq!(
        split_package_specifier("@s/p/x"),
        Some(("@s/p", "/x".to_string()))
    );
    assert_eq!(split_package_specifier("@s"), None);
    assert_eq!(split_package_specifier("node_modules/x"), None);
}

#[test]
fn relative_specifiers_inside_a_package_probe_extensions() {
    let fs = MemoryFs::with(&[
        ("/project/node_modules/a/lib/helper.d.ts", ""),
        ("/project/node_modules/a/lib/dir/index.d.ts", ""),
    ]);
    let resolver = resolver(&fs, ModuleResolution::Node10);
    let from = Path::new("/project/node_modules/a/lib");
    assert_eq!(
        resolver.resolve_relative(from, "./helper").unwrap().path,
        PathBuf::from("/project/node_modules/a/lib/helper.d.ts")
    );
    assert_eq!(
        resolver.resolve_relative(from, "./helper.js").unwrap().path,
        PathBuf::from("/project/node_modules/a/lib/helper.d.ts")
    );
    assert_eq!(
        resolver.resolve_relative(from, "./dir").unwrap().path,
        PathBuf::from("/project/node_modules/a/lib/dir/index.d.ts")
    );
    assert!(resolver.resolve_relative(from, "./absent").is_err());
    // A relative path cannot climb out of the roots.
    assert!(matches!(
        resolver.resolve_relative(from, "../../../../etc/passwd"),
        Err(ResolveError::NotFound { .. } | ResolveError::OutsideRoots { .. })
    ));
}

#[test]
fn a_symlinked_package_inside_the_roots_resolves_to_its_real_path() {
    let fs = MemoryFs::with(&[
        (
            "/project/.store/a-1.0.0/package.json",
            r#"{"types":"a.d.ts"}"#,
        ),
        ("/project/.store/a-1.0.0/a.d.ts", ""),
    ]);
    fs.link("/project/node_modules/a", "/project/.store/a-1.0.0");
    let found = resolve(&fs, ModuleResolution::Node10, "a", ImportMode::Import).unwrap();
    assert_eq!(found.path, PathBuf::from("/project/.store/a-1.0.0/a.d.ts"));
    assert_eq!(
        found.package.unwrap().root,
        PathBuf::from("/project/.store/a-1.0.0")
    );
}

#[test]
fn a_symlinked_package_outside_the_roots_is_refused_without_falling_through() {
    let fs = MemoryFs::with(&[
        ("/elsewhere/a/index.d.ts", "stolen"),
        ("/project/node_modules/a/index.d.ts", "legit-but-farther"),
        ("/project/src/node_modules/placeholder/index.d.ts", ""),
    ]);
    fs.link("/project/src/node_modules/a", "/elsewhere/a");
    let error = resolve(&fs, ModuleResolution::Node10, "a", ImportMode::Import).unwrap_err();
    assert!(
        matches!(error, ResolveError::OutsideRoots { .. }),
        "{error}"
    );
}

#[test]
fn a_symlinked_file_that_leaves_the_roots_is_refused() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"types":"leak.d.ts"}"#,
        ),
        ("/secret/leak.d.ts", "secret"),
    ]);
    fs.link("/project/node_modules/a/leak.d.ts", "/secret/leak.d.ts");
    let error = resolve(&fs, ModuleResolution::Node10, "a", ImportMode::Import).unwrap_err();
    assert!(
        matches!(error, ResolveError::OutsideRoots { .. }),
        "{error}"
    );
}

#[test]
fn an_importer_outside_the_roots_is_refused() {
    let fs = MemoryFs::with(&[("/project/node_modules/a/index.d.ts", "")]);
    let error = resolver(&fs, ModuleResolution::Node10)
        .resolve(Path::new("/other/src"), "a", ImportMode::Import)
        .unwrap_err();
    assert!(
        matches!(error, ResolveError::OutsideRoots { .. }),
        "{error}"
    );
}

#[test]
fn a_malformed_package_json_is_an_error_naming_the_file() {
    let fs = MemoryFs::with(&[
        ("/project/node_modules/a/package.json", "{not json"),
        ("/project/node_modules/a/index.d.ts", ""),
    ]);
    let error = resolve(&fs, ModuleResolution::Node10, "a", ImportMode::Import).unwrap_err();
    assert!(error.to_string().contains("package.json"), "{error}");
}

#[test]
fn the_fingerprint_is_stable_and_tracks_configuration() {
    let fs = exports_fs();
    let one = resolver(&fs, ModuleResolution::Node16);
    let two = resolver(&fs, ModuleResolution::Node16);
    for resolver in [&one, &two] {
        resolver
            .resolve(Path::new("/project/src"), "a", ImportMode::Import)
            .unwrap();
    }
    assert_eq!(one.fingerprint(), two.fingerprint());
    let bundler = resolver(&fs, ModuleResolution::Bundler);
    bundler
        .resolve(Path::new("/project/src"), "a", ImportMode::Import)
        .unwrap();
    assert_ne!(one.fingerprint(), bundler.fingerprint());
    assert!(one.fingerprint().starts_with("bts-packages-"));
}

#[test]
fn revalidation_notices_a_closer_package_a_changed_manifest_and_a_repointed_link() {
    let base = || {
        MemoryFs::with(&[
            (
                "/project/node_modules/a/package.json",
                r#"{"types":"a.d.ts"}"#,
            ),
            ("/project/node_modules/a/a.d.ts", ""),
        ])
    };

    let fs = base();
    let resolver = resolver(&fs, ModuleResolution::Node10);
    resolver
        .resolve(Path::new("/project/src"), "a", ImportMode::Import)
        .unwrap();
    assert!(resolver.revalidate());
    let before = resolver.fingerprint();

    // A nearer node_modules now provides the package.
    fs.write("/project/src/node_modules/a/index.d.ts", "");
    assert!(!resolver.revalidate());
    fs.remove("/project/src/node_modules/a/index.d.ts");
    assert!(resolver.revalidate());

    // The manifest changes.
    fs.write(
        "/project/node_modules/a/package.json",
        r#"{"types":"b.d.ts"}"#,
    );
    assert!(!resolver.revalidate());
    fs.write(
        "/project/node_modules/a/package.json",
        r#"{"types":"a.d.ts"}"#,
    );
    assert!(resolver.revalidate());
    assert_eq!(resolver.fingerprint(), before);

    // A candidate probed earlier in the order appears.
    let fs = MemoryFs::with(&[("/project/node_modules/a/index.d.ts", "")]);
    let resolver = self::resolver(&fs, ModuleResolution::Node10);
    resolver
        .resolve(Path::new("/project/src"), "a", ImportMode::Import)
        .unwrap();
    fs.write("/project/node_modules/a/index.ts", "");
    assert!(!resolver.revalidate());

    // A symlink is repointed.
    let fs = MemoryFs::with(&[
        ("/project/.s/one/index.d.ts", ""),
        ("/project/.s/two/index.d.ts", ""),
    ]);
    fs.link("/project/node_modules/a", "/project/.s/one");
    let resolver = self::resolver(&fs, ModuleResolution::Node10);
    resolver
        .resolve(Path::new("/project/src"), "a", ImportMode::Import)
        .unwrap();
    assert!(resolver.revalidate());
    fs.link("/project/node_modules/a", "/project/.s/two");
    assert!(!resolver.revalidate());
}

#[test]
fn module_resolution_names_round_trip() {
    for resolution in [
        ModuleResolution::Node10,
        ModuleResolution::Node16,
        ModuleResolution::Bundler,
    ] {
        assert_eq!(
            ModuleResolution::parse(resolution.as_str()),
            Some(resolution)
        );
    }
    assert_eq!(
        ModuleResolution::parse("NodeNext"),
        Some(ModuleResolution::Node16)
    );
    assert_eq!(ModuleResolution::parse("classic"), None);
}

#[test]
fn a_manifest_field_cannot_lead_out_of_its_package() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"types":"../../../outside/x.d.ts","typings":"/etc/passwd","main":"../b/index.js"}"#,
        ),
        ("/outside/x.d.ts", ""),
        ("/project/node_modules/b/index.d.ts", ""),
        ("/project/node_modules/a/index.d.ts", ""),
    ]);
    // Every escaping field is skipped; the package's own `index` is used, and
    // nothing outside the package was probed.
    let resolver = resolver(&fs, ModuleResolution::Node10);
    let found = resolver
        .resolve(Path::new("/project/src"), "a", ImportMode::Import)
        .unwrap();
    assert_eq!(
        found.path,
        PathBuf::from("/project/node_modules/a/index.d.ts")
    );
    assert!(!resolver.fingerprint().contains("/outside"));
}

#[test]
fn every_error_has_a_message_naming_what_went_wrong() {
    let errors = [
        ResolveError::InvalidSpecifier("../x".to_string()),
        ResolveError::NotFound {
            specifier: "a".to_string(),
            searched: vec![PathBuf::from("/p")],
        },
        ResolveError::OutsideRoots {
            specifier: "a".to_string(),
            path: PathBuf::from("/elsewhere"),
        },
        ResolveError::InvalidPackageJson {
            path: PathBuf::from("/p/package.json"),
            message: "bad".to_string(),
        },
        ResolveError::ExportsNotDefined {
            package: "a".to_string(),
            subpath: "./x".to_string(),
        },
        ResolveError::JavaScriptOnly {
            specifier: "a".to_string(),
            path: PathBuf::from("/p/index.js"),
        },
        ResolveError::ImportsNotDefined {
            specifier: "#x".to_string(),
        },
    ];
    let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
    for (message, needle) in messages.iter().zip([
        "not a valid",
        "nothing is installed",
        "outside the authorized",
        "package.json",
        "does not export",
        "only to JavaScript",
        "no package `imports`",
    ]) {
        assert!(message.contains(needle), "{message}");
    }
}

#[test]
fn resolved_packages_and_config_are_reported_and_fs_errors_surface() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/a/package.json",
            r#"{"version":"1.0.0","types":"a.d.ts"}"#,
        ),
        ("/project/node_modules/a/a.d.ts", ""),
        (
            "/project/node_modules/b/package.json",
            r#"{"main":"index.js"}"#,
        ),
    ]);
    let resolver = resolver(&fs, ModuleResolution::Node10);
    resolver
        .resolve(Path::new("/project/src"), "a", ImportMode::Import)
        .unwrap();
    assert_eq!(resolver.resolved_packages().len(), 1);
    assert_eq!(resolver.config().resolution, ModuleResolution::Node10);
    assert!(resolver.within_roots(Path::new("/project/x")));
    assert!(!resolver.within_roots(Path::new("/other")));
    // A package with a manifest and no files at all is simply not found.
    assert!(matches!(
        resolver.resolve(Path::new("/project/src"), "b", ImportMode::Import),
        Err(ResolveError::NotFound { .. })
    ));
    // The real file system reports the same errors.
    let os = PackageResolver::new(
        OsPackageFs,
        PackageResolverConfig {
            roots: vec![std::env::temp_dir()],
            resolution: ModuleResolution::Bundler,
            custom_conditions: vec![],
        },
    );
    assert!(os
        .resolve(
            &std::env::temp_dir(),
            "definitely-not-installed-xyz",
            ImportMode::Require
        )
        .is_err());
    assert!(OsPackageFs
        .read_to_string(Path::new("/definitely/not/here"))
        .is_err());
    assert!(!OsPackageFs.is_file(Path::new("/definitely/not/here")));
    assert!(!OsPackageFs.is_dir(Path::new("/definitely/not/here")));
    assert!(OsPackageFs
        .canonicalize(Path::new("/definitely/not/here"))
        .is_err());
}

#[test]
fn exports_edge_shapes_are_resolved_or_refused_precisely() {
    let fs = MemoryFs::with(&[
        (
            "/project/node_modules/mixed/package.json",
            r#"{"exports":{".":"./i.d.ts","default":"./d.d.ts"}}"#,
        ),
        (
            "/project/node_modules/arr/package.json",
            r#"{"exports":["./nope.d.ts","./yes.d.ts"]}"#,
        ),
        ("/project/node_modules/arr/yes.d.ts", ""),
        (
            "/project/node_modules/bad/package.json",
            r#"{"exports":{".":42}}"#,
        ),
        (
            "/project/node_modules/nul/package.json",
            r#"{"exports":null,"types":"t.d.ts"}"#,
        ),
        ("/project/node_modules/nul/t.d.ts", ""),
        (
            "/project/node_modules/pat/package.json",
            r#"{"exports":{"./a/*":"./x/*.d.ts","./a/b/*":"./y/*.d.ts"}}"#,
        ),
        ("/project/node_modules/pat/y/c.d.ts", ""),
    ]);
    let at = |name: &str| resolve(&fs, ModuleResolution::Node16, name, ImportMode::Import);
    assert!(matches!(
        at("mixed"),
        Err(ResolveError::InvalidPackageJson { .. })
    ));
    assert!(at("arr").is_ok());
    assert!(matches!(
        at("bad"),
        Err(ResolveError::InvalidPackageJson { .. })
    ));
    // `exports: null` does not count as an exports map, so `types` is used.
    assert!(at("nul").is_ok());
    // The pattern with the longer prefix wins.
    assert_eq!(
        path_of(at("pat/a/b/c")),
        PathBuf::from("/project/node_modules/pat/y/c.d.ts")
    );
}
