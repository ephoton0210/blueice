// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Installed packages through `bluetsc` (J.4.2, J.4.3): an owner-configured
//! `moduleResolution` resolves bare specifiers from the dependency tree the
//! owner authorized, on a real disk with real symlinks, and nothing outside it.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("bluetsc-packages-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("app/src")).unwrap();
        Self { root }
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn config(&self, extra: serde_json::Value) {
        let mut config = serde_json::json!({
            "entries": ["src/main.ts"],
            "outDir": "dist",
            "declaration": true,
            "moduleResolution": "node16",
        });
        for (key, value) in extra.as_object().unwrap() {
            config[key] = value.clone();
        }
        self.write(
            "app/bluetsc.json",
            &serde_json::to_string_pretty(&config).unwrap(),
        );
    }

    fn run(&self, command: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .current_dir(self.root.join("app"))
            .args([command, "--config", "bluetsc.json"])
            .output()
            .unwrap()
    }

    fn manifest(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.root.join("app/dist/bluetsc.manifest.json")).unwrap())
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

const MAIN: &str = "import { shout } from \"typed-pkg\";\nimport { pad } from \"typed-pkg/strings\";\nimport type { Legacy } from \"legacy\";\nexport const out: string = shout(\"a\") + pad(\"b\");\nexport function keep(value: Legacy): Legacy { return value; }\n";

fn install_packages(project: &Project) {
    project.write(
        "app/node_modules/typed-pkg/package.json",
        r#"{"name":"typed-pkg","version":"2.1.0","exports":{".":{"types":"./index.d.ts","default":"./index.js"},"./strings":{"types":"./strings.d.ts"}}}"#,
    );
    project.write(
        "app/node_modules/typed-pkg/index.d.ts",
        "export declare function shout(text: string): string;\n",
    );
    project.write(
        "app/node_modules/typed-pkg/strings.d.ts",
        "export declare function pad(text: string): string;\n",
    );
    project.write(
        "app/node_modules/@types/legacy/index.d.ts",
        "export interface Legacy { id: number }\n",
    );
    project.write("app/src/main.ts", MAIN);
}

#[test]
fn installed_packages_resolve_through_exports_and_types_packages_without_being_emitted() {
    let project = Project::new("ok");
    install_packages(&project);
    project.config(serde_json::json!({}));
    let built = project.run("build");
    assert!(built.status.success(), "{}", stderr(&built));
    let javascript = fs::read_to_string(project.root.join("app/dist/src/main.js")).unwrap();
    // The runtime keeps the package's own specifier, and no package file is emitted.
    assert!(javascript.contains("\"typed-pkg\""), "{javascript}");
    assert!(javascript.contains("\"typed-pkg/strings\""), "{javascript}");
    assert!(!project.root.join("app/dist/node_modules").exists());
    let manifest = project.manifest();
    let packages = &manifest["packageResolution"];
    assert_eq!(packages["moduleResolution"], "node16");
    assert!(packages["fingerprint"]
        .as_str()
        .unwrap()
        .starts_with("bts-packages-"));
    let names: Vec<(String, bool)> = packages["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| {
            (
                package["name"].as_str().unwrap().to_string(),
                package["typesPackage"].as_bool().unwrap(),
            )
        })
        .collect();
    assert!(
        names.contains(&("typed-pkg".to_string(), false)),
        "{names:?}"
    );
    assert!(names.contains(&("legacy".to_string(), true)), "{names:?}");
    assert_eq!(
        packages["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"] == "typed-pkg")
            .unwrap()["version"],
        "2.1.0"
    );
}

#[test]
fn a_type_error_against_a_package_declaration_is_reported() {
    let project = Project::new("typeerror");
    install_packages(&project);
    project.write(
        "app/src/main.ts",
        "import type { Legacy } from \"legacy\";\nexport const ok: Legacy = { id: 1 };\nexport const bad: Legacy = 5;\n",
    );
    project.config(serde_json::json!({}));
    let checked = project.run("check");
    assert!(!checked.status.success());
    assert!(stderr(&checked).contains("BTS3"), "{}", stderr(&checked));
}

#[test]
fn nothing_is_installed_implicitly_and_a_missing_package_is_an_error() {
    let project = Project::new("missing");
    project.write(
        "app/src/main.ts",
        "import { x } from \"not-installed\";\nexport const y = x;\n",
    );
    project.config(serde_json::json!({}));
    let checked = project.run("check");
    assert!(!checked.status.success());
    assert!(
        stderr(&checked).contains("nothing is installed implicitly"),
        "{}",
        stderr(&checked)
    );
    assert!(!project.root.join("app/node_modules").exists());
}

#[test]
fn without_a_configured_module_resolution_bare_specifiers_stay_refused() {
    let project = Project::new("unconfigured");
    install_packages(&project);
    project.config(serde_json::json!({"moduleResolution": null}));
    let checked = project.run("check");
    assert!(!checked.status.success());
    assert!(
        stderr(&checked).contains("bare specifier"),
        "{}",
        stderr(&checked)
    );
}

#[cfg(unix)]
#[test]
fn a_package_symlinked_outside_every_authorized_root_is_refused() {
    let project = Project::new("escape");
    project.write(
        "outside/typed-pkg/index.d.ts",
        "export declare function shout(text: string): string;\n",
    );
    project.write(
        "app/src/main.ts",
        "import { shout } from \"typed-pkg\";\nexport const o = shout(\"a\");\n",
    );
    fs::create_dir_all(project.root.join("app/node_modules")).unwrap();
    std::os::unix::fs::symlink(
        project.root.join("outside/typed-pkg"),
        project.root.join("app/node_modules/typed-pkg"),
    )
    .unwrap();
    project.config(serde_json::json!({}));
    let checked = project.run("check");
    assert!(!checked.status.success());
    assert!(
        stderr(&checked).contains("outside the authorized package roots"),
        "{}",
        stderr(&checked)
    );

    // The owner may then authorize that tree explicitly.
    project.config(serde_json::json!({"packageRoots": ["../outside"]}));
    let allowed = project.run("build");
    assert!(allowed.status.success(), "{}", stderr(&allowed));
    assert_eq!(project.manifest()["packageResolution"]["externalRoots"], 1);
}

#[cfg(unix)]
#[test]
fn a_symlinked_package_inside_the_root_resolves_to_its_real_directory() {
    let project = Project::new("inside");
    project.write(
        "app/.store/typed-pkg-1/index.d.ts",
        "export declare function shout(text: string): string;\n",
    );
    project.write(
        "app/src/main.ts",
        "import { shout } from \"typed-pkg\";\nexport const o = shout(\"a\");\n",
    );
    fs::create_dir_all(project.root.join("app/node_modules")).unwrap();
    std::os::unix::fs::symlink(
        project.root.join("app/.store/typed-pkg-1"),
        project.root.join("app/node_modules/typed-pkg"),
    )
    .unwrap();
    project.config(serde_json::json!({}));
    let built = project.run("build");
    assert!(built.status.success(), "{}", stderr(&built));
}

#[test]
fn a_hoisted_dependency_tree_needs_its_root_authorized() {
    let project = Project::new("hoisted");
    project.write(
        "node_modules/typed-pkg/index.d.ts",
        "export declare function shout(text: string): string;\n",
    );
    project.write(
        "app/src/main.ts",
        "import { shout } from \"typed-pkg\";\nexport const o = shout(\"a\");\n",
    );
    project.config(serde_json::json!({}));
    assert!(!project.run("check").status.success());
    project.config(serde_json::json!({"packageRoots": [".."]}));
    let built = project.run("build");
    assert!(built.status.success(), "{}", stderr(&built));
}

#[test]
fn bundler_and_node10_strategies_and_conditions_are_selectable() {
    let project = Project::new("strategies");
    project.write(
        "app/node_modules/dual/package.json",
        r#"{"name":"dual","types":"legacy.d.ts","exports":{".":{"development":"./dev.d.ts","default":"./prod.d.ts"}}}"#,
    );
    for file in ["legacy.d.ts", "dev.d.ts", "prod.d.ts"] {
        project.write(
            &format!("app/node_modules/dual/{file}"),
            &format!(
                "export interface Which {{ {}: number }}\n",
                file.replace(".d.ts", "")
            ),
        );
    }
    // The program type-checks only against the declaration the strategy selects.
    for (extra, expected) in [
        (
            serde_json::json!({"moduleResolution": "node10"}),
            "legacy.d.ts",
        ),
        (
            serde_json::json!({"moduleResolution": "bundler"}),
            "prod.d.ts",
        ),
        (
            serde_json::json!({"moduleResolution": "bundler", "customConditions": ["development"]}),
            "dev.d.ts",
        ),
    ] {
        project.write(
            "app/src/main.ts",
            &format!(
                "import type {{ Which }} from \"dual\";\nexport const w: Which = {{ {} : 1 }};\n",
                expected.replace(".d.ts", "")
            ),
        );
        project.config(extra.clone());
        let checked = project.run("check");
        assert!(checked.status.success(), "{extra}: {}", stderr(&checked));
        for wrong in ["legacy.d.ts", "dev.d.ts", "prod.d.ts"] {
            if wrong == expected {
                continue;
            }
            project.write(
                "app/src/main.ts",
                &format!("import type {{ Which }} from \"dual\";\nexport const w: Which = {{ {}: 1 }};\n", wrong.replace(".d.ts", "")),
            );
            assert!(
                !project.run("check").status.success(),
                "{extra} accepted {wrong}"
            );
        }
    }
    project.config(serde_json::json!({"moduleResolution": "classic"}));
    assert!(stderr(&project.run("check")).contains("unsupported moduleResolution"));
    project.config(serde_json::json!({"moduleResolution": null, "packageRoots": ["."]}));
    assert!(stderr(&project.run("check")).contains("need a moduleResolution"));
}

#[test]
fn configuration_and_resolved_files_change_the_fingerprint() {
    let project = Project::new("fingerprint");
    install_packages(&project);
    project.config(serde_json::json!({}));
    assert!(project.run("build").status.success());
    let first = project.manifest();
    project.config(serde_json::json!({"moduleResolution": "bundler"}));
    assert!(project.run("build").status.success());
    let second = project.manifest();
    assert_ne!(
        first["packageResolution"]["fingerprint"],
        second["packageResolution"]["fingerprint"]
    );
    assert_ne!(first["fingerprint"], second["fingerprint"]);
    // A package manifest edit that does not change the chosen file still
    // changes what the resolution depended on.
    project.write(
        "app/node_modules/typed-pkg/package.json",
        r#"{"name":"typed-pkg","version":"2.1.1","exports":{".":{"types":"./index.d.ts"},"./strings":{"types":"./strings.d.ts"}}}"#,
    );
    assert!(project.run("build").status.success());
    let third = project.manifest();
    assert_ne!(
        second["packageResolution"]["fingerprint"],
        third["packageResolution"]["fingerprint"]
    );
    assert_ne!(second["fingerprint"], third["fingerprint"]);
}

#[test]
fn a_commonjs_project_resolves_an_installed_package_and_a_commonjs_module_and_runs() {
    let project = Project::new("commonjs");
    install_packages(&project);
    project.write(
        "app/node_modules/typed-pkg/package.json",
        r#"{"name":"typed-pkg","version":"2.1.0","exports":{".":{"types":"./index.d.ts","default":"./index.js"},"./strings":{"types":"./strings.d.ts","default":"./strings.js"}}}"#,
    );
    project.write(
        "app/node_modules/typed-pkg/index.js",
        "exports.shout = (text) => text.toUpperCase() + \"!\";\n",
    );
    project.write(
        "app/node_modules/typed-pkg/strings.js",
        "exports.pad = (text) => \"[\" + text + \"]\";\n",
    );
    project.write(
        "app/src/lib.ts",
        "function twice(n: number): number { return n * 2; }\nexport = twice;\n",
    );
    project.write(
        "app/src/main.ts",
        "import twice = require(\"./lib\");\nimport { shout } from \"typed-pkg\";\nimport { pad } from \"typed-pkg/strings\";\nconsole.log(shout(\"hi\"), pad(\"x\"), twice(21));\n",
    );
    project.config(serde_json::json!({"module": "commonjs", "moduleResolution": "node16"}));
    let built = project.run("build");
    assert!(built.status.success(), "{}", stderr(&built));
    let main = fs::read_to_string(project.root.join("app/dist/src/main.js")).unwrap();
    assert!(main.contains("require(\"typed-pkg\")"), "{main}");
    assert!(main.contains("require(\"./lib\")"), "{main}");
    assert_eq!(project.manifest()["module"], "commonjs");
    if Command::new("node").arg("--version").output().is_ok() {
        // The output runs where the owner's runtime provides the package.
        fs::write(
            project.root.join("app/dist/package.json"),
            r#"{"type":"commonjs"}"#,
        )
        .unwrap();
        let ran = Command::new("node")
            .arg(project.root.join("app/dist/src/main.js"))
            .env("NODE_PATH", project.root.join("app/node_modules"))
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&ran.stdout).trim(),
            "HI! [x] 42",
            "{}",
            String::from_utf8_lossy(&ran.stderr)
        );
    }
}
