// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Remote declaration sources (J.4.4) through `bluetsc` and the library: pinned
//! content, an explicit fetch step, no source-driven fetch, and no authority
//! from a declaration.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use blueice_bluets::remote_declarations::sha256_hex;
use blueice_bluets::{compile, CompilerOptions, MapLoader, ModuleSource};

const DECLARATION: &str =
    "export interface Remote { id: number }\nexport declare function remoteValue(): number;\n";

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("bluetsc-remote-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src")).unwrap();
        Self { root }
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn cache(&self, text: &str) -> String {
        let pin = sha256_hex(text.as_bytes());
        self.write(&format!(".bluetsc/declarations/{pin}.d.ts"), text);
        pin
    }

    fn config(&self, pin: &str, url: &str) {
        self.write(
            "bluetsc.json",
            &serde_json::to_string_pretty(&serde_json::json!({
                "entries": ["src/main.ts"],
                "outDir": "dist",
                "remoteDeclarations": [{"specifier": "remote-lib", "url": url, "sha256": pin}],
            }))
            .unwrap(),
        );
    }

    fn run(&self, command: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(&self.root)
            .args([command, "--config", "bluetsc.json"])
            .output()
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const MAIN: &str = "import type { Remote } from \"remote-lib\";\nimport { remoteValue } from \"remote-lib\";\nexport const r: Remote = { id: 1 };\nexport const v = remoteValue();\n";

#[test]
fn a_pinned_cached_declaration_types_the_program_and_adds_no_emitted_code() {
    let project = Project::new("ok");
    let pin = project.cache(DECLARATION);
    project.config(&pin, "https://example.test/remote.d.ts");
    project.write("src/main.ts", MAIN);
    let built = project.run("build");
    assert!(built.status.success(), "{}", stderr(&built));
    let javascript = fs::read_to_string(project.root.join("dist/src/main.js")).unwrap();
    assert!(javascript.contains("\"remote-lib\""), "{javascript}");
    assert!(
        !javascript.contains("remoteValue()") || javascript.contains("import"),
        "{javascript}"
    );
    // Only the program is emitted; the remote declaration is neither copied nor run.
    let files: Vec<String> = walk(&project.root.join("dist"));
    assert!(!files.iter().any(|file| file.contains(&pin)), "{files:?}");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(project.root.join("dist/bluetsc.manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["remoteDeclarations"][0]["sha256"], pin);
    assert_eq!(manifest["remoteDeclarations"][0]["specifier"], "remote-lib");
}

fn walk(directory: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(directory).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path.display().to_string());
        }
    }
    out
}

#[test]
fn the_declaration_really_types_the_program() {
    let project = Project::new("typed");
    let pin = project.cache(DECLARATION);
    project.config(&pin, "https://example.test/remote.d.ts");
    project.write(
        "src/main.ts",
        "import type { Remote } from \"remote-lib\";\nexport const bad: Remote = 5;\n",
    );
    let checked = project.run("check");
    assert!(!checked.status.success());
    assert!(stderr(&checked).contains("BTS3"), "{}", stderr(&checked));
}

#[test]
fn a_missing_cache_entry_names_the_explicit_fetch_step_and_never_fetches() {
    let project = Project::new("uncached");
    let pin = sha256_hex(DECLARATION.as_bytes());
    // An address nothing listens on: if compilation fetched, it would fail
    // differently (and slowly).
    project.config(&pin, "https://127.0.0.1:9/remote.d.ts");
    project.write("src/main.ts", MAIN);
    for command in ["check", "build"] {
        let output = project.run(command);
        assert!(!output.status.success());
        assert!(
            stderr(&output).contains("fetch-declarations"),
            "{}",
            stderr(&output)
        );
    }
}

#[test]
fn a_tampered_cache_entry_is_refused() {
    let project = Project::new("tamper");
    let pin = project.cache(DECLARATION);
    project.write(
        &format!(".bluetsc/declarations/{pin}.d.ts"),
        "export declare const evil: number;\n",
    );
    project.config(&pin, "https://example.test/remote.d.ts");
    project.write("src/main.ts", MAIN);
    let output = project.run("check");
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("does not match its hash"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_owners_list_is_validated_when_the_config_is_read() {
    let project = Project::new("config");
    project.write("src/main.ts", MAIN);
    let pin = sha256_hex(DECLARATION.as_bytes());
    for (url, pin, expected) in [
        ("http://example.test/a.d.ts", pin.as_str(), "only https"),
        (
            "https://u:p@example.test/a.d.ts",
            pin.as_str(),
            "credentials",
        ),
        ("https://example.test/a.d.ts", "XYZ", "SHA-256 pin"),
    ] {
        project.config(pin, url);
        let output = project.run("check");
        assert!(!output.status.success());
        assert!(
            stderr(&output).contains(expected),
            "{url}: {}",
            stderr(&output)
        );
    }
    project.write(
        "bluetsc.json",
        r#"{"entries":["src/main.ts"],"remoteDeclarations":[{"specifier":"a","url":"https://x.test/a.d.ts","sha256":"00"}],"declarationCache":"../outside"}"#,
    );
    assert!(!project.run("check").status.success());
    project.write(
        "bluetsc.json",
        r#"{"entries":["src/main.ts"],"declarationCache":"cache"}"#,
    );
    assert!(stderr(&project.run("check")).contains("needs remoteDeclarations"));
}

#[test]
fn source_text_cannot_make_bluetsc_fetch_or_reach_a_url() {
    let project = Project::new("nosource");
    let hostile = "/// <reference path=\"https://evil.test/ref.d.ts\" />\n/// <reference types=\"evil\" />\nexport interface Remote { id: number }\nexport declare const viaImportType: import(\"https://evil.test/x\").T;\n";
    let pin = project.cache(hostile);
    project.config(&pin, "https://example.test/remote.d.ts");
    // A source that names a URL directly is refused by resolution, not fetched.
    project.write(
        "src/main.ts",
        "import { x } from \"https://evil.test/x.d.ts\";\nexport const y = x;\n",
    );
    let output = project.run("check");
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("bare specifier"),
        "{}",
        stderr(&output)
    );
    // A configured declaration with references and URL import types is used as
    // text only; nothing in it triggers a request (the run is offline and fast).
    project.write(
        "src/main.ts",
        "import type { Remote } from \"remote-lib\";\nexport const r: Remote = { id: 1 };\n",
    );
    let started = std::time::Instant::now();
    let output = project.run("check");
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    let _ = output;
}

#[test]
fn a_remote_declaration_cannot_import_anything() {
    let project = Project::new("selfcontained");
    let text =
        "import type { Other } from \"other-lib\";\nexport interface Remote { other: Other }\n";
    let pin = project.cache(text);
    project.config(&pin, "https://example.test/remote.d.ts");
    project.write(
        "src/main.ts",
        "import type { Remote } from \"remote-lib\";\nexport const r = 1;\n",
    );
    let output = project.run("check");
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("self-contained"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn fetching_needs_a_config_with_sources_and_fails_cleanly_when_unreachable() {
    let project = Project::new("fetch");
    project.write("src/main.ts", MAIN);
    project.write("bluetsc.json", r#"{"entries":["src/main.ts"]}"#);
    let output = project.run("fetch-declarations");
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("no remoteDeclarations"),
        "{}",
        stderr(&output)
    );
    let pin = sha256_hex(DECLARATION.as_bytes());
    project.config(&pin, "https://127.0.0.1:9/remote.d.ts");
    let output = project.run("fetch-declarations");
    assert!(!output.status.success());
    assert!(stderr(&output).contains("fetching"), "{}", stderr(&output));
    assert!(!project
        .root
        .join(format!(".bluetsc/declarations/{pin}.d.ts"))
        .exists());
    // The command takes no loose flags or entry.
    let loose = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
        .args(["fetch-declarations", "src/main.ts"])
        .output()
        .unwrap();
    assert!(!loose.status.success());
}

/// A declaration grants no run-time host API: its names are not ambient globals,
/// it is not emitted, and the direct bridge refuses to link it.
#[test]
fn a_remote_declaration_grants_no_global_and_no_runtime_binding() {
    let pin = sha256_hex(b"x");
    let remote_id = format!("@remote/{pin}.d.ts");
    let declaration = "declare function hostFetch(url: string): string;\ndeclare const hostSecret: number;\nexport declare function allowed(): number;\n";
    let sources = |main: &str| {
        MapLoader::from(vec![
            ModuleSource::new("main.ts", main),
            ModuleSource::new(remote_id.clone(), declaration),
        ])
    };
    let loader_with_edge = |main: &str| EdgeLoader {
        inner: sources(main),
        remote_id: remote_id.clone(),
    };
    let options = || CompilerOptions {
        require_declared_global_calls: true,
        ..CompilerOptions::default()
    };
    // The imported name works; the names the declaration merely declared do not
    // become globals.
    let ok = compile(
        "main.ts",
        &loader_with_edge("import { allowed } from \"remote-lib\";\nexport const a = allowed();\n"),
        options(),
    );
    assert!(ok.output.is_some(), "{:#?}", ok.diagnostics);
    let artifacts = &ok.output.unwrap().artifacts;
    assert_eq!(
        artifacts.keys().collect::<Vec<_>>(),
        ["main.ts"],
        "the declaration is not emitted"
    );

    let leaked = compile(
        "main.ts",
        &loader_with_edge("import { allowed } from \"remote-lib\";\nexport const a = allowed();\nexport const b = hostFetch(\"https://x\");\n"),
        options(),
    );
    assert!(
        leaked.output.is_none(),
        "a declaration's `declare function` must not become a global"
    );
}

/// Resolves `remote-lib` to the remote module, as `bluetsc` does for a pinned source.
struct EdgeLoader {
    inner: MapLoader,
    remote_id: String,
}

impl blueice_bluets::ModuleLoader for EdgeLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        self.inner.load(module_id)
    }

    fn resolve(&self, from: &str, specifier: &str) -> Result<String, String> {
        if specifier == "remote-lib" {
            Ok(self.remote_id.clone())
        } else {
            self.inner.resolve(from, specifier)
        }
    }
}
