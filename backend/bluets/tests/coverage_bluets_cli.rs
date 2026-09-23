// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Process-boundary coverage for the `bluetsc` command line: argument
//! parsing, project/config validation, resolver confinement, diagnostics
//! rendering and atomic publishing.  Every test drives the real compiled
//! binary with real arguments and inspects its exit status and streams.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A scratch directory that removes itself, even when an assertion fails.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "blueice-bluetsc-coverage-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Scratch(path)
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }

    fn write(&self, relative: &str, contents: &str) -> &Scratch {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
        self
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).unwrap()
    }

    /// Runs `bluetsc` with the scratch directory as the working directory.
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_bluetsc"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap();
        Run {
            success: output.status.success(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Run {
    success: bool,
    stdout: String,
    stderr: String,
}

impl Run {
    #[track_caller]
    fn assert_failure(&self, message: &str) {
        assert!(!self.success, "expected failure, stdout: {}", self.stdout);
        assert!(
            self.stderr.contains(message),
            "stderr should contain `{message}`, got: {}",
            self.stderr
        );
        assert!(self.stdout.is_empty(), "stdout: {}", self.stdout);
    }

    #[track_caller]
    fn assert_success(&self) {
        assert!(self.success, "expected success, stderr: {}", self.stderr);
        assert!(self.stderr.is_empty(), "stderr: {}", self.stderr);
    }
}

const GOOD: &str = "export const answer: number = 42;\n";

#[test]
fn help_is_printed_to_stdout_with_success() {
    let scratch = Scratch::new();
    for args in [
        vec!["--help"],
        vec!["-h"],
        vec!["check", "main.ts", "--help"],
        vec!["build", "main.ts", "-h"],
    ] {
        let run = scratch.run(&args);
        run.assert_success();
        assert!(
            run.stdout.starts_with("Usage:\n  bluetsc check"),
            "{args:?}"
        );
        assert!(run.stdout.contains("bluetsc build --config <bluetsc.json>"));
        assert!(run.stdout.contains("Config fields: entries, projectRoot"));
    }
}

#[test]
fn malformed_command_lines_fail_with_a_message_and_the_usage_text() {
    let scratch = Scratch::new();
    let cases: &[(&[&str], &str)] = &[
        (&[], "a command is required"),
        (
            &["compile", "main.ts"],
            "unknown command `compile`; expected `check` or `build`",
        ),
        (
            &["check"],
            "an entry .ts file or --config <file> is required",
        ),
        (&["check", "--config"], "--config requires a JSON file"),
        (
            &["check", "--config", "a.json", "--source-map"],
            "`--config` owns project settings; unsupported extra argument `--source-map`",
        ),
        (
            &["build", "main.ts"],
            "build requires --out-dir <directory>",
        ),
        (
            &["check", "main.ts", "--out-dir", "dist"],
            "--out-dir is valid only with build",
        ),
        (
            &["check", "main.ts", "--target", "es2019"],
            "unsupported target `es2019`; expected es2020 or es2022",
        ),
        (
            &["check", "main.ts", "--runtime-policy", "loose"],
            "unsupported runtime policy `loose`; expected transpile-only, checked, or strict-runtime",
        ),
        (
            &["check", "main.ts", "--project-root"],
            "--project-root requires a value",
        ),
        (
            &["build", "main.ts", "--out-dir"],
            "--out-dir requires a value",
        ),
        (
            &["check", "main.ts", "--target"],
            "--target requires a value",
        ),
        (
            &["check", "main.ts", "--frobnicate"],
            "unrecognized argument `--frobnicate`",
        ),
    ];
    for (args, message) in cases {
        let run = scratch.run(args);
        assert!(!run.success, "{args:?} should fail");
        assert!(
            run.stderr
                .starts_with(&format!("bluetsc: {message}\n\nUsage:")),
            "{args:?}: {}",
            run.stderr
        );
        assert!(run.stdout.is_empty(), "{args:?}: {}", run.stdout);
    }
}

#[test]
fn check_uses_the_entry_directory_as_the_default_project_root() {
    let scratch = Scratch::new();
    scratch
        .write(
            "main.ts",
            "import { helper } from './lib/helper.ts';\nexport const out: number = helper;\n",
        )
        .write("lib/helper.ts", "export const helper: number = 7;\n");
    let run = scratch.run(&["check", "main.ts"]);
    run.assert_success();
    assert!(
        run.stdout
            .starts_with("checked 1 entry point(s), 2 module(s), fingerprint bts-project-"),
        "{}",
        run.stdout
    );
    // The fingerprint is deterministic for identical input.
    assert_eq!(scratch.run(&["check", "main.ts"]).stdout, run.stdout);
    // ...and depends on the compiler options in effect.
    let other = scratch.run(&["check", "main.ts", "--target", "es2020"]);
    other.assert_success();
    assert_ne!(other.stdout, run.stdout);
}

#[test]
fn explicit_entry_and_project_root_problems_are_reported_before_compiling() {
    let scratch = Scratch::new();
    scratch
        .write("project/main.ts", GOOD)
        .write("project/types.d.ts", "export interface T { id: string }\n")
        .write("outside.ts", GOOD)
        .write("plain.txt", "not a directory");

    scratch
        .run(&["check", "missing.ts"])
        .assert_failure("bluetsc: cannot read entry missing.ts:");
    scratch
        .run(&["check", "project/main.ts", "--project-root", "nowhere"])
        .assert_failure("bluetsc: cannot access --project-root nowhere:");
    scratch
        .run(&["check", "project/main.ts", "--project-root", "plain.txt"])
        .assert_failure("is not a directory");
    scratch
        .run(&["check", "outside.ts", "--project-root", "project"])
        .assert_failure("is outside project root");
    scratch
        .run(&["check", "project/types.d.ts"])
        .assert_failure("is a .d.ts declaration module, not an executable entry");
}

#[test]
fn compile_diagnostics_are_rendered_one_per_line_and_fail_the_command() {
    let scratch = Scratch::new();
    scratch.write("main.ts", "export const value: string = 1;\n");
    let run = scratch.run(&["check", "main.ts"]);
    assert!(!run.success);
    assert!(run.stdout.is_empty());
    let line = run.stderr.lines().next().unwrap();
    let mut parts = line.splitn(5, ':');
    assert_eq!(parts.next(), Some("main.ts"));
    let start: usize = parts.next().unwrap().parse().unwrap();
    let end: usize = parts.next().unwrap().parse().unwrap();
    assert!(start < end && end <= "export const value: string = 1;\n".len());
    assert_eq!(parts.next().unwrap().trim(), "BTS3003");
    assert!(!parts.next().unwrap().trim().is_empty());

    // A parse error is likewise reported with its code.
    scratch.write("syntax.ts", "const = ;\n");
    let run = scratch.run(&["check", "syntax.ts"]);
    assert!(!run.success);
    assert!(run.stderr.contains("BTS1000"), "{}", run.stderr);
}

#[test]
fn the_file_loader_only_resolves_closed_project_ts_sources() {
    let scratch = Scratch::new();
    scratch
        .write(
            "project/main.ts",
            "import './absent.ts';\nexport const a: number = 1;\n",
        )
        .write("project/data.ts.txt", "x")
        .write("project/data.json", "{}")
        .write(
            "project/json.ts",
            "import './data.json';\nexport const j: number = 1;\n",
        )
        .write(
            "project/escape.ts",
            "import '../outside.ts';\nexport const e: number = 1;\n",
        )
        .write(
            "project/bare.ts",
            "import 'left-pad';\nexport const b: number = 1;\n",
        )
        .write("outside.ts", GOOD);

    let expectations = [
        ("main.ts", "cannot resolve `./absent.ts` from `main.ts`"),
        ("json.ts", "is not a supported .ts, .tsx, or .d.ts source file"),
        (
            "escape.ts",
            "specifier `../outside.ts` resolves outside the declared project root",
        ),
        (
            "bare.ts",
            "bare specifier `left-pad` is unsupported; add an exact or trailing-slash `imports` mapping",
        ),
    ];
    for (entry, message) in expectations {
        let run = scratch.run(&[
            "check",
            &format!("project/{entry}"),
            "--project-root",
            "project",
        ]);
        assert!(!run.success, "{entry}");
        assert!(run.stdout.is_empty(), "{entry}");
        assert!(run.stderr.contains("BTS2000"), "{entry}: {}", run.stderr);
        assert!(run.stderr.contains(message), "{entry}: {}", run.stderr);
    }
}

#[test]
fn build_flags_are_recorded_in_the_manifest() {
    let scratch = Scratch::new();
    scratch.write(
        "main.ts",
        "export function id(value: number): number { return value; }\n",
    );
    let run = scratch.run(&[
        "build",
        "main.ts",
        "--out-dir",
        "dist",
        "--target",
        "es2020",
        "--runtime-policy",
        "transpile-only",
        "--source-map",
        "--declaration",
    ]);
    run.assert_success();
    assert!(
        run.stdout
            .starts_with("built 1 entry point(s), 1 module(s), fingerprint bts-project-"),
        "{}",
        run.stdout
    );
    // A relative `--out-dir` is anchored at the working directory.
    let manifest: serde_json::Value =
        serde_json::from_str(&scratch.read("dist/bluetsc.manifest.json")).unwrap();
    assert_eq!(manifest["target"], "es2020");
    assert_eq!(manifest["runtimePolicy"], "transpile-only");
    assert_eq!(manifest["sourceMap"], true);
    assert_eq!(manifest["declaration"], true);
    assert_eq!(manifest["entries"], serde_json::json!(["main.js"]));
    assert_eq!(manifest["declarationModules"], serde_json::json!([]));
    assert_eq!(manifest["imports"], serde_json::json!({}));
    // No configured imports means no import map is published.
    assert!(!scratch.path("dist/bluetsc.importmap.json").exists());
    assert!(scratch
        .read("dist/main.js")
        .ends_with("\n//# sourceMappingURL=main.js.map\n"));
    assert!(scratch.path("dist/main.js.map").is_file());
    assert!(scratch.path("dist/main.d.ts").is_file());
}

#[test]
fn build_refuses_an_output_that_is_the_project_root_or_a_regular_file() {
    let scratch = Scratch::new();
    scratch
        .write("project/main.ts", GOOD)
        .write("blocker", "i am a file");

    scratch
        .run(&["build", "project/main.ts", "--out-dir", "project"])
        .assert_failure("output directory must not replace the project root");
    assert_eq!(scratch.read("project/main.ts"), GOOD);

    scratch
        .run(&[
            "build",
            "project/main.ts",
            "--project-root",
            "project",
            "--out-dir",
            "blocker",
        ])
        .assert_failure("blocker exists and is not a directory");
    assert_eq!(scratch.read("blocker"), "i am a file");
    // The staging directory is cleaned up after the refusal.
    let leftovers: Vec<_> = fs::read_dir(&scratch.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with(".bluetsc-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn build_creates_missing_output_parents_and_replaces_an_existing_output_atomically() {
    let scratch = Scratch::new();
    scratch.write("project/main.ts", GOOD);
    let args = [
        "build",
        "project/main.ts",
        "--project-root",
        "project",
        "--out-dir",
        "deep/er/dist",
    ];
    scratch.run(&args).assert_success();
    assert!(scratch.path("deep/er/dist/main.js").is_file());

    // A stale file in the previous output does not survive a rebuild.
    scratch.write("deep/er/dist/stale.js", "old");
    scratch.run(&args).assert_success();
    assert!(!scratch.path("deep/er/dist/stale.js").exists());
    assert!(scratch.path("deep/er/dist/main.js").is_file());
    let leftovers: Vec<_> = fs::read_dir(scratch.path("deep/er"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(leftovers, ["dist"]);
}

fn config_project(scratch: &Scratch) {
    scratch
        .write("src/main.ts", GOOD)
        .write("src/lib.ts", "export const lib: number = 1;\n")
        .write(
            "src/shared/model.ts",
            "export interface Model { id: string }\n",
        )
        .write("types/api.d.ts", "export interface Api { id: string }\n");
}

#[test]
fn configs_that_do_not_load_or_parse_are_rejected() {
    let scratch = Scratch::new();
    config_project(&scratch);
    scratch
        .run(&["check", "--config", "absent.json"])
        .assert_failure("bluetsc: cannot read config absent.json:");
    for (text, message) in [
        ("{", "invalid config"),
        ("[]", "invalid config"),
        (
            "{\"entries\": [\"src/main.ts\"], \"bogus\": true}",
            "unknown field `bogus`",
        ),
        ("{\"entries\": \"src/main.ts\"}", "invalid config"),
        ("{}", "missing field `entries`"),
        (
            "{\"entries\": []}",
            "config `entries` must contain at least one .ts entry",
        ),
    ] {
        scratch.write("bluetsc.json", text);
        scratch
            .run(&["check", "--config", "bluetsc.json"])
            .assert_failure(message);
    }
    // A build needs an output directory from the configuration.
    scratch.write("bluetsc.json", "{\"entries\": [\"src/main.ts\"]}");
    scratch
        .run(&["build", "--config", "bluetsc.json"])
        .assert_failure("build requires --out-dir <directory> or config outDir");
    scratch
        .run(&["check", "--config", "bluetsc.json"])
        .assert_success();
}

#[test]
fn config_project_root_and_entries_are_confined_to_the_config_directory() {
    let scratch = Scratch::new();
    config_project(&scratch);
    scratch.write("plain.txt", "file");
    let cases: &[(&str, &str)] = &[
        (
            r#"{"entries": ["main.ts"], "projectRoot": "../src"}"#,
            "must stay beneath the config directory",
        ),
        (
            r#"{"entries": ["main.ts"], "projectRoot": "src/../src"}"#,
            "must stay beneath the config directory",
        ),
        (
            r#"{"entries": ["main.ts"], "projectRoot": "nowhere"}"#,
            "cannot access config projectRoot under",
        ),
        (
            r#"{"entries": ["main.ts"], "projectRoot": "plain.txt"}"#,
            "is not a directory",
        ),
        (
            r#"{"entries": ["absent.ts"], "projectRoot": "src"}"#,
            "cannot read configured entry:",
        ),
        (
            r#"{"entries": ["../types/api.d.ts"], "projectRoot": "src"}"#,
            "is outside project root",
        ),
        (
            r#"{"entries": ["main.ts"], "projectRoot": "src", "outDir": "../dist"}"#,
            "config outDir `../dist` must stay beneath projectRoot",
        ),
        (
            r#"{"entries": ["src/main.ts"], "outDir": ""}"#,
            "config outDir `` must stay beneath projectRoot",
        ),
        (
            r#"{"entries": ["src/main.ts"], "outDir": "./dist"}"#,
            "must stay beneath projectRoot",
        ),
    ];
    for (text, message) in cases {
        scratch.write("bluetsc.json", text);
        let run = scratch.run(&["check", "--config", "bluetsc.json"]);
        assert!(!run.success, "{text}");
        assert!(run.stderr.contains(message), "{text}: {}", run.stderr);
        assert!(run.stdout.is_empty(), "{text}");
    }

    // A declaration module is not an executable entry, even when configured.
    scratch.write("bluetsc.json", r#"{"entries": ["types/api.d.ts"]}"#);
    scratch
        .run(&["check", "--config", "bluetsc.json"])
        .assert_failure("is a .d.ts declaration module, not an executable entry");
}

#[test]
fn config_project_root_relocates_entries_and_output() {
    let scratch = Scratch::new();
    config_project(&scratch);
    scratch.write(
        "bluetsc.json",
        r#"{"entries": ["main.ts", "lib.ts", "main.ts"], "projectRoot": "src", "outDir": "out/js"}"#,
    );
    let run = scratch.run(&["build", "--config", "bluetsc.json"]);
    run.assert_success();
    // Duplicate entries are collapsed.
    assert!(
        run.stdout
            .starts_with("built 2 entry point(s), 2 module(s)"),
        "{}",
        run.stdout
    );
    assert!(scratch.path("src/out/js/main.js").is_file());
    assert!(scratch.path("src/out/js/lib.js").is_file());
    let manifest: serde_json::Value =
        serde_json::from_str(&scratch.read("src/out/js/bluetsc.manifest.json")).unwrap();
    assert_eq!(
        manifest["entries"],
        serde_json::json!(["lib.js", "main.js"])
    );
    assert_eq!(manifest["target"], "es2022");
    assert_eq!(manifest["runtimePolicy"], "checked");
}

#[test]
fn config_target_and_runtime_policy_are_validated_and_applied() {
    let scratch = Scratch::new();
    config_project(&scratch);
    for (text, message) in [
        (
            r#"{"entries": ["src/main.ts"], "target": "es5"}"#,
            "unsupported target `es5`; expected es2020 or es2022",
        ),
        (
            r#"{"entries": ["src/main.ts"], "runtimePolicy": "reckless"}"#,
            "unsupported runtime policy `reckless`; expected transpile-only, checked, or strict-runtime",
        ),
    ] {
        scratch.write("bluetsc.json", text);
        scratch
            .run(&["check", "--config", "bluetsc.json"])
            .assert_failure(message);
    }
    for (policy, expected) in [("transpile-only", "transpile-only"), ("checked", "checked")] {
        scratch.write(
            "bluetsc.json",
            &format!(
                r#"{{"entries": ["src/main.ts"], "outDir": "dist", "target": "es2020", "runtimePolicy": "{policy}"}}"#
            ),
        );
        scratch
            .run(&["build", "--config", "bluetsc.json"])
            .assert_success();
        let manifest: serde_json::Value =
            serde_json::from_str(&scratch.read("dist/bluetsc.manifest.json")).unwrap();
        assert_eq!(manifest["runtimePolicy"], expected);
        assert_eq!(manifest["target"], "es2020");
    }
    scratch.write(
        "bluetsc.json",
        r#"{"entries": ["src/main.ts"], "outDir": "strict-dist", "runtimePolicy": "strict-runtime"}"#,
    );
    scratch
        .run(&["build", "--config", "bluetsc.json"])
        .assert_failure(
            "strict-runtime build requires the versioned runtime boundary helper, which standalone BlueTSC does not install",
        );
    assert!(
        !scratch.path("strict-dist").exists(),
        "a strict-runtime build without its helper must not publish an artifact"
    );
}

#[test]
fn config_import_maps_are_validated() {
    let scratch = Scratch::new();
    scratch
        .write("project/main.ts", GOOD)
        .write("project/src/lib.ts", "export const lib: number = 1;\n")
        .write(
            "project/src/shared/model.ts",
            "export interface Model { id: string }\n",
        )
        .write("elsewhere/x.ts", GOOD);
    let cases: &[(&str, &str)] = &[
        (r#"{"": "src/lib.ts"}"#, "invalid import-map key ``"),
        (r#"{".rel": "src/lib.ts"}"#, "invalid import-map key `.rel`"),
        (r#"{"/abs": "src/lib.ts"}"#, "invalid import-map key `/abs`"),
        (
            r#"{"@gone": "src/gone.ts"}"#,
            "cannot resolve import-map target for `@gone`:",
        ),
        (r#"{"@out": "../elsewhere/x.ts"}"#, "import-map target"),
        (
            r#"{"@shared/": "src/lib.ts"}"#,
            "import-map prefix `@shared/` must target a project-root-confined directory",
        ),
        (
            r#"{"@shared": "src/shared"}"#,
            "import-map key `@shared` must target a project-root-confined source file",
        ),
    ];
    for (imports, message) in cases {
        // `elsewhere` is a sibling of the project root, so `../elsewhere`
        // must be rejected as escaping it.
        scratch.write(
            "project/bluetsc.json",
            &format!(r#"{{"entries": ["main.ts"], "imports": {imports}}}"#),
        );
        let run = scratch.run(&["check", "--config", "project/bluetsc.json"]);
        assert!(!run.success, "{imports}");
        assert!(run.stderr.contains(message), "{imports}: {}", run.stderr);
        assert!(run.stdout.is_empty(), "{imports}");
    }
}

#[test]
fn import_maps_resolve_exact_and_longest_prefix_specifiers() {
    let scratch = Scratch::new();
    scratch
        .write(
            "main.ts",
            "import { lib } from '@lib';\nimport type { Model } from '@app/models/model.ts';\nimport type { Special } from '@app/models/special/special.ts';\nexport const value: number = lib;\nexport type Both = [Model, Special];\n",
        )
        .write("src/lib.ts", "export const lib: number = 1;\n")
        .write("src/models/model.ts", "export interface Model { id: string }\n")
        .write("special/special.ts", "export interface Special { tag: string }\n")
        .write(
            "bluetsc.json",
            r#"{
  "entries": ["main.ts"],
  "outDir": "dist",
  "imports": {
    "@lib": "src/lib.ts",
    "@app/": "src",
    "@app/models/special/": "special"
  }
}"#,
        );
    scratch
        .run(&["check", "--config", "bluetsc.json"])
        .assert_success();
    scratch
        .run(&["build", "--config", "bluetsc.json"])
        .assert_success();
    assert!(scratch.path("dist/src/lib.js").is_file());
    assert!(scratch.path("dist/src/models/model.js").is_file());
    assert!(scratch.path("dist/special/special.js").is_file());

    let import_map: serde_json::Value =
        serde_json::from_str(&scratch.read("dist/bluetsc.importmap.json")).unwrap();
    assert_eq!(
        import_map,
        serde_json::json!({"imports": {
            "@app/": "./src/",
            "@app/models/special/": "./special/",
            "@lib": "./src/lib.js",
        }})
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&scratch.read("dist/bluetsc.manifest.json")).unwrap();
    assert_eq!(manifest["imports"], import_map["imports"]);
}

#[test]
fn a_config_build_can_be_pointed_at_a_config_in_a_subdirectory() {
    let scratch = Scratch::new();
    scratch.write("app/src/main.ts", GOOD).write(
        "app/bluetsc.json",
        r#"{"entries": ["src/main.ts"], "outDir": "build"}"#,
    );
    let run = scratch.run(&["build", "--config", "app/bluetsc.json"]);
    run.assert_success();
    assert!(scratch.path("app/build/src/main.js").is_file());
    // Nothing is written beside the working directory.
    assert!(!scratch.path("build").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_entries_and_imports_cannot_escape_the_project_root() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    scratch
        .write(
            "project/main.ts",
            "import './link.ts';\nexport const a: number = 1;\n",
        )
        .write("outside/secret.ts", "export const secret: number = 1;\n");
    symlink(
        scratch.path("outside/secret.ts"),
        scratch.path("project/link.ts"),
    )
    .unwrap();
    let run = scratch.run(&["check", "project/main.ts", "--project-root", "project"]);
    assert!(!run.success);
    assert!(
        run.stderr
            .contains("resolves outside the declared project root"),
        "{}",
        run.stderr
    );

    // A symlinked entry that leaves the root is refused up front.
    symlink(
        scratch.path("outside/secret.ts"),
        scratch.path("project/entry-link.ts"),
    )
    .unwrap();
    scratch
        .run(&[
            "check",
            "project/entry-link.ts",
            "--project-root",
            "project",
        ])
        .assert_failure("is outside project root");
}

#[test]
fn absolute_paths_are_accepted_for_entry_root_and_output() {
    let scratch = Scratch::new();
    scratch.write("project/main.ts", GOOD);
    let entry = scratch.path("project/main.ts");
    let root = scratch.path("project");
    let out = scratch.path("absolute-out");
    let run = scratch.run(&[
        "build",
        entry.to_str().unwrap(),
        "--project-root",
        root.to_str().unwrap(),
        "--out-dir",
        out.to_str().unwrap(),
    ]);
    run.assert_success();
    assert!(fs::read_to_string(out.join("main.js"))
        .unwrap()
        .contains("export const answer"));
    assert!(Path::new(&out).join("bluetsc.manifest.json").is_file());
}
