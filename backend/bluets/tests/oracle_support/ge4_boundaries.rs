// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Output options retain canonical owner authority, work and publication limits.
use super::*;
use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, DiagnosticCode, EcmaTarget, IncrementalCompiler,
    MapLoader, ModuleKind, ModuleLoader, ModuleSource, NewLine, RuntimePolicy,
};
use serde_json::{json, Value};
use std::fs;

const ENTRY: &str = "memory:///main.ts";
fn row(form: &str, mask: usize) -> Value {
    let id = format!("ge4-{form}-commonjs-{mask:02}");
    input()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap()
        .clone()
}
fn policy() -> CompilerOptions {
    CompilerOptions {
        target: EcmaTarget::Es2022,
        module_kind: ModuleKind::CommonJs,
        libraries: Some(vec![EcmaTarget::Es2022]),
        checking: Some(CheckingOptions::default()),
        use_define_for_class_fields: Some(false),
        declaration: true,
        ..CompilerOptions::default()
    }
}
struct Owner(MapLoader);
impl Owner {
    fn new(row: &Value, revised: bool) -> Self {
        Self(MapLoader::from(
            row["files"]
                .as_object()
                .unwrap()
                .iter()
                .filter(|(name, _)| name.ends_with(".ts"))
                .map(|(name, text)| {
                    ModuleSource::new(
                        format!("memory:///{name}"),
                        format!(
                            "{}{}",
                            text.as_str().unwrap(),
                            if revised && name == "main.ts" {
                                "\n// owner revision\n"
                            } else {
                                ""
                            }
                        ),
                    )
                }),
        ))
    }
}
impl ModuleLoader for Owner {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        self.0.load(id)
    }
    fn resolve(&self, _: &str, specifier: &str) -> Result<String, String> {
        if specifier == "./base.js" {
            Ok("memory:///base.ts".into())
        } else {
            Err("this source has no grant for another module".into())
        }
    }
}
fn success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn canonical_source_and_output_authority() {
    let case = row("fresh-imported-alias", 0);
    let owner = Owner::new(&case, false);
    let allowed = compile(ENTRY, &owner, policy());
    assert!(!allowed.has_errors(), "{:?}", allowed.diagnostics);
    let refused = compile("memory:///ungranted.ts", &owner, policy());
    assert!(refused.output.is_none());
    assert!(refused
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ModuleNotFound));
    let temp = Temporary::new("source-authority");
    let root = temp.project(&case);
    success(&command(&root));
    let before = outputs(&root);
    #[cfg(unix)]
    {
        let secret = temp.0.join("ungranted-provider.ts");
        fs::write(&secret, case["files"]["base.ts"].as_str().unwrap()).unwrap();
        fs::remove_file(root.join("base.ts")).unwrap();
        std::os::unix::fs::symlink(&secret, root.join("base.ts")).unwrap();
        let denied = command(&root);
        assert!(!denied.status.success());
        assert!(!String::from_utf8_lossy(&denied.stdout).contains("ungranted-provider"));
        assert_eq!(outputs(&root), before);
        assert_eq!(
            fs::read_to_string(&secret).unwrap(),
            case["files"]["base.ts"].as_str().unwrap()
        );
    }
    let mut escaping = row("nested-annotated-object", 0);
    let outside = temp.0.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("owner.txt"), "preserve").unwrap();
    escaping["options"]["outDir"] = json!("../outside");
    let root = temp.project(&escaping);
    assert!(!command(&root).status.success());
    assert_eq!(
        fs::read_to_string(outside.join("owner.txt")).unwrap(),
        "preserve"
    );
}
#[test]
fn failed_output_option_publication_remains_atomic() {
    let temp = Temporary::new("atomic-options");
    let case = row("class-inferred-field", 0);
    let root = temp.project(&case);
    success(&command(&root));
    let before = outputs(&root);
    let rejected = row("class-inferred-field", 2);
    assert_eq!(rejected["accepts"], false);
    fs::write(
        root.join("tsconfig.json"),
        serde_json::to_string_pretty(
            &json!({"compilerOptions":rejected["options"],"files":[rejected["entry"]]}),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!command(&root).status.success());
    assert_eq!(outputs(&root), before);
}
#[test]
fn nested_declaration_work_is_charged() {
    let case = row("nested-annotated-object", 0);
    let owner = Owner::new(&case, false);
    let normal = compile(ENTRY, &owner, policy());
    assert!(!normal.has_errors(), "{:?}", normal.diagnostics);
    for bound in ["bytes", "tokens", "depth", "maps"] {
        let mut options = policy();
        match bound {
            "bytes" => options.limits.max_total_source_bytes = 10,
            "tokens" => options.limits.parser.max_tokens = 10,
            "depth" => options.limits.parser.max_type_depth = 0,
            "maps" => {
                options.source_map = true;
                options.limits.max_source_map_segments = 0;
            }
            _ => unreachable!(),
        }
        let denied = compile(ENTRY, &owner, options);
        assert!(denied.output.is_none(), "{bound}");
        assert!(
            denied
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ResourceLimit),
            "{bound}: {:?}",
            denied.diagnostics
        );
    }
}
#[test]
fn option_source_and_cache_identity_are_distinct() {
    let case = row("nested-annotated-object", 0);
    let owner = Owner::new(&case, false);
    let mut compiler = IncrementalCompiler::new();
    let first = compiler.compile(ENTRY, &owner, policy());
    assert!(
        !first.compilation.has_errors(),
        "{:?}",
        first.compilation.diagnostics
    );
    let repeated = compiler.compile(ENTRY, &owner, policy());
    assert!(repeated.cache_hit);
    assert_eq!(repeated.compilation.output, first.compilation.output);
    for flag in ["strip", "isolated", "newline", "bom", "comments", "fields"] {
        let mut options = policy();
        match flag {
            "strip" => options.strip_internal = true,
            "isolated" => options.isolated_declarations = true,
            "newline" => options.new_line = NewLine::CrLf,
            "bom" => options.emit_bom = true,
            "comments" => options.remove_comments = true,
            "fields" => options.use_define_for_class_fields = Some(true),
            _ => unreachable!(),
        }
        let switched = compiler.compile(ENTRY, &owner, options.clone());
        assert!(!switched.cache_hit, "{flag}");
        assert!(
            !switched.compilation.has_errors(),
            "{flag}: {:?}",
            switched.compilation.diagnostics
        );
        assert_ne!(
            switched.compilation.project_fingerprint,
            first.compilation.project_fingerprint
        );
        assert_eq!(
            switched.compilation.output,
            compile(ENTRY, &owner, options).output,
            "{flag}"
        );
    }
    let revised = Owner::new(&case, true);
    let changed = compiler.compile(ENTRY, &revised, policy());
    assert!(!changed.cache_hit);
    assert_ne!(
        changed.compilation.project_fingerprint,
        first.compilation.project_fingerprint
    );
    assert_eq!(
        changed.compilation.output,
        compile(ENTRY, &revised, policy()).output
    );
}
#[test]
fn declarations_do_not_authorize_runtime_values() {
    let source = "interface Host { new(): { value: number }; } export const result: number = new Host().value;";
    for strip in [false, true] {
        let mut options = policy();
        options.strip_internal = strip;
        let denied = compile(
            ENTRY,
            &MapLoader::from([ModuleSource::new(ENTRY, source)]),
            options,
        );
        assert!(denied.output.is_none());
        assert!(denied
            .diagnostics
            .iter()
            .any(|d| d.typescript.as_ref().is_some_and(|t| t.code == 2693)));
    }
}
#[test]
fn helper_map_and_default_denied_policy_are_retained() {
    // A literal with header-like bytes cannot impersonate a retained comment.
    for (docs, retained) in [("", 1), ("/** Public. */\n", 2)] {
        let source = format!("/*! G-E4 pinned */\n{docs}function choose(value: string): string; function choose(value: string): string {{ return value; }} export const text: string = \"/*! G-E4 pinned */\";");
        let literal_owner = MapLoader::from([ModuleSource::new(ENTRY, source)]);
        let mut literal_options = policy();
        literal_options.remove_comments = true;
        let literal = compile(ENTRY, &literal_owner, literal_options);
        assert!(!literal.has_errors(), "{:?}", literal.diagnostics);
        let javascript = &literal.output.as_ref().unwrap().artifacts[ENTRY].javascript;
        assert_eq!(javascript.matches("/*! G-E4 pinned */").count(), retained);
        assert!(javascript.contains("\"/*! G-E4 pinned */\""));
    }
    let source = "/** public */ export const result: number = 1;\n";
    let owner = MapLoader::from([ModuleSource::new(ENTRY, source)]);
    let mut options = policy();
    options.runtime_policy = RuntimePolicy::StrictRuntime;
    options.source_map = true;
    options.emit_bom = true;
    options.new_line = NewLine::CrLf;
    let allowed = compile(ENTRY, &owner, options.clone());
    assert!(!allowed.has_errors(), "{:?}", allowed.diagnostics);
    let artifact = &allowed.output.as_ref().unwrap().artifacts[ENTRY];
    assert!(artifact.source_map.is_some());
    assert!(artifact.javascript.starts_with('\u{feff}'));
    assert!(artifact.javascript.contains("\r\n"));
    assert!(!artifact.javascript.contains("require("));
    options.limits.max_source_map_segments = 0;
    let denied = compile(ENTRY, &owner, options);
    assert!(denied.output.is_none());
    assert!(denied
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ResourceLimit));
}
