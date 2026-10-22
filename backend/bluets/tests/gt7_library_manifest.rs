// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! G-T7 freezes library precision independently from runtime capability policy.
#[path = "oracle_support/gt7_library.rs"]
mod support;
use blueice_bluets::{standard_library, CompilerOptions, DiagnosticCode, EcmaTarget, ModuleSource};
use serde_json::json;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn every_manifest_entry_retains_its_native_pairs_and_consumers() {
    let (input, reference) = support::records();
    assert_eq!(reference["typescript"], "5.9.3");
    assert_eq!(input["forms"].as_array().unwrap().len(), 217);
    assert_eq!(reference["cases"].as_array().unwrap().len(), 434);
    let mut rows = reference["cases"].as_array().unwrap().iter();
    let mut ids = BTreeSet::new();
    let mut areas = BTreeSet::new();
    let mut executions = 0;
    for form in input["forms"].as_array().unwrap() {
        assert!(ids.insert(form["id"].as_str().unwrap()));
        areas.insert(form["area"].as_str().unwrap());
        for verdict in ["accept", "reject"] {
            let row = rows.next().unwrap();
            assert_eq!(
                row["id"],
                format!("{}-{verdict}", form["id"].as_str().unwrap())
            );
            assert_eq!(
                row["files"]["entry.ts"],
                format!(
                    "// UTF-16 control: 😀\n{}\n{}\n",
                    form["prefix"].as_str().unwrap(),
                    form[verdict].as_str().unwrap()
                )
            );
            let mut expected = input["options"].clone();
            if let Some(overrides) = form["options"].as_object() {
                expected.as_object_mut().unwrap().extend(overrides.clone());
            }
            assert_eq!(row["options"], expected);
            assert_eq!(row["area"], form["area"]);
            assert_eq!(row["accepts"], verdict == "accept");
            if verdict == "accept" {
                assert_eq!(row["declarations"].as_object().unwrap().len(), 1);
                for kind in ["accept", "reject"] {
                    let consumer = &row["consumers"][kind];
                    assert_eq!(
                        consumer["source"],
                        format!(
                            "// UTF-16 consumer: 😀\n{}\n",
                            form[format!("consumer_{kind}")].as_str().unwrap()
                        )
                    );
                    assert_eq!(
                        consumer["diagnostics"].as_array().unwrap().is_empty(),
                        kind == "accept"
                    );
                }
                if row.get("runtime").is_some() {
                    executions += 1;
                }
            }
        }
    }
    assert!(rows.next().is_none());
    assert_eq!(areas.len(), 19);
    assert_eq!(executions, 5);
    let mut profiles = reference["profile_controls"].as_array().unwrap().iter();
    for control in input["profile_controls"].as_array().unwrap() {
        for eligible in [true, false] {
            let row = profiles.next().unwrap();
            assert_eq!(
                row["id"],
                format!(
                    "{}-{}",
                    control["id"].as_str().unwrap(),
                    if eligible { "eligible" } else { "below" }
                )
            );
            assert_eq!(
                row["files"]["entry.ts"],
                format!(
                    "// UTF-16 profile: 😀\n{}\n",
                    control["source"].as_str().unwrap()
                )
            );
            let mut expected = input["options"].clone();
            expected["target"] = json!("ES5");
            expected["lib"] = json!([control[if eligible { "edition" } else { "below" }].clone()]);
            assert_eq!(row["options"], expected);
            assert_eq!(row["accepts"], eligible);
        }
    }
    assert!(profiles.next().is_none());
    assert_eq!(input["owner_controls"], reference["owner_controls"]);
}

#[test]
fn library_manifest_matches_exact_native_results_and_downstream_diagnostics() {
    support::outputs();
}

#[test]
fn owner_replacement_and_runtime_policy_do_not_merge_library_authority() {
    let (input, _) = support::records();
    let controls = input["owner_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 5);
    let control = &controls[0];
    let source = format!(
        "{} const witness: 'owner' = result;",
        control["source"].as_str().unwrap()
    );
    let mut options = support::options(&json!({}));
    options.ambient_declaration_modules.push(ModuleSource::new(
        "memory:///owner.d.ts",
        control["owner"].as_str().unwrap(),
    ));
    assert!(!support::checked(&source, options.clone()).has_errors());
    assert!(support::checked("Array.from(1);", options).has_errors());
    assert!(support::checked(&source, support::options(&json!({}))).has_errors());
    let local = format!("{} {source}", control["owner"].as_str().unwrap());
    assert!(!support::checked(&local, support::options(&json!({}))).has_errors());
    let control = &controls[1];
    let mut options = support::options(&json!({}));
    options.require_declared_global_calls = true;
    let static_namespace = support::checked(
        "export function format(value: Intl.NumberFormat) { return value.format(1); }",
        options.clone(),
    );
    assert!(
        !static_namespace.has_errors(),
        "{:?}",
        static_namespace.diagnostics
    );
    assert!(support::checked(
        "export const value = new Intl.NumberFormat();",
        options.clone()
    )
    .has_errors());
    assert!(support::checked(control["source"].as_str().unwrap(), options.clone()).has_errors());
    options.ambient_declaration_modules.push(ModuleSource::new(
        "memory:///runtime-owner.d.ts",
        control["owner"].as_str().unwrap(),
    ));
    assert!(!support::checked(control["source"].as_str().unwrap(), options).has_errors());
    let declaration = "export declare const result: number;";
    let consumer = "import type * as api from './entry.js'; export const copied = api.result;";
    for checking in [Some(blueice_bluets::CheckingOptions::default()), None] {
        let mut options = support::options(&json!({}));
        options.checking = checking;
        let result = support::checked_files(
            "consumer.ts",
            &[("entry.d.ts", declaration), ("consumer.ts", consumer)],
            options,
        );
        assert!(result.output.is_none());
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.typescript.as_ref().is_some_and(|ts| ts.code == 1361)));
    }
    let shadow = "import type * as api from './entry.js'; export function read(api: {result: string}) { const value: typeof api.result = 'local'; return value; }";
    let result = support::checked_files(
        "consumer.ts",
        &[("entry.d.ts", declaration), ("consumer.ts", shadow)],
        support::options(&json!({})),
    );
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    let source = controls[4]["source"].as_str().unwrap();
    assert!(!support::checked(source, support::options(&json!({}))).has_errors());
    let mut limited = support::options(&json!({}));
    limited.limits.max_type_expansions = 1;
    let result = support::checked(source, limited);
    assert!(result.output.is_none());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == DiagnosticCode::ResourceLimit && d.typescript.is_none()));
}

#[test]
fn structural_library_inference_keeps_rejection_and_owner_boundaries() {
    // Reuse the old frozen observations: these are regressions, not new case
    // denominator entries or newly generated native expectations.
    let old: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/diagnostics/reference.json")).unwrap();
    for (name, source) in [
        (
            "infer-return-generic-member-argument-error",
            include_str!(
                "fixtures/typescript_oracle/infer-return-generic-member-argument-error/main.ts"
            ),
        ),
        (
            "lib-error-constructor-error",
            include_str!("fixtures/typescript_oracle/lib-error-constructor-error/main.ts"),
        ),
        (
            "lib-promise-method-error",
            include_str!("fixtures/typescript_oracle/lib-promise-method-error/main.ts"),
        ),
        (
            "lib-regexp-constructor-error",
            include_str!("fixtures/typescript_oracle/lib-regexp-constructor-error/main.ts"),
        ),
        (
            "lib-array-map-too-many",
            include_str!("fixtures/typescript_oracle/lib-array-map-too-many/main.ts"),
        ),
        (
            "lib-array-map-wrong-type",
            include_str!("fixtures/typescript_oracle/lib-array-map-wrong-type/main.ts"),
        ),
        (
            "lib-filter-callback-type-error",
            include_str!("fixtures/typescript_oracle/lib-filter-callback-type-error/main.ts"),
        ),
    ] {
        let row = old["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["entry"].as_str() == Some(&format!("{name}/main.ts")))
            .unwrap();
        let flags = row["flags"].as_array().unwrap();
        let target = flags.windows(2).find(|pair| pair[0] == "--target").unwrap()[1]
            .as_str()
            .unwrap();
        let result = support::checked(
            source,
            CompilerOptions {
                target: support::edition(target),
                ..CompilerOptions::default()
            },
        );
        let diagnostic = result
            .diagnostics
            .first()
            .and_then(|diagnostic| diagnostic.typescript.as_ref())
            .unwrap();
        assert_eq!(
            diagnostic.code as u64,
            row["first"]["code"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(
            diagnostic.message,
            row["first"]["message"].as_str().unwrap(),
            "{name}"
        );
        assert_eq!(
            diagnostic.span.start as u64,
            row["first"]["start"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(
            (diagnostic.span.end - diagnostic.span.start) as u64,
            row["first"]["length"].as_u64().unwrap(),
            "{name}"
        );
        assert!(diagnostic.related_information.is_empty(), "{name}");
    }
    let options = || support::options(&json!({}));
    for source in [
        "declare const value: Promise<string>; const bad: PromiseLike<number> = value;",
        "declare const value: PromiseLike<string>; const bad: PromiseLike<number> = value;",
        "type PromiseLike<T> = {custom: T}; declare const value: Promise<string>; const bad: PromiseLike<string> = value;",
        "Object.freeze({value: 1}).value = 2;",
        "Reflect.apply(1, undefined, [1]);",
        "Reflect.construct(1, [0]);",
        "declare const source: { [Symbol.iterator](): Iterator<string> }; const bad: number[] = Array.from(source);",
    ] {
        assert!(support::checked(source, options()).has_errors(), "unexpected acceptance: {source}");
    }
    for source in [
        "declare function reduce<U>(callback: (previous: U) => U, initial: U): U; const result: string = reduce(value => value + 'x', '');",
        "declare function freeze<T extends { [key: string]: U }, U extends string | number>(value: T): T; const result: {value: 1} = freeze({value: 1});",
        "declare function tuple<T extends readonly any[]>(value: Readonly<T>): T; const result: [number, number] = tuple([1, 2]);",
    ] {
        let result = support::checked(source, options());
        assert!(!result.has_errors(), "{source}: {:?}", result.diagnostics);
    }
}

#[test]
fn library_identity_binds_exact_content_without_publishing_owner_inputs() {
    let (input, _) = support::records();
    let source = input["owner_controls"][2]["source"].as_str().unwrap();
    let mut options = support::options(&json!({}));
    options.limits.max_modules = 1;
    options.limits.max_total_source_bytes = source.len();
    let result = support::checked(source, options);
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    assert_eq!(result.project.modules.len(), 1);
    assert!(result
        .checked
        .unwrap()
        .modules
        .values()
        .flat_map(|m| &m.symbols)
        .all(|s| s.span.module == "memory:///entry.ts"));
    assert_eq!(result.output.unwrap().artifacts.len(), 1);
    let identity = standard_library::identity(EcmaTarget::Es2022);
    assert_eq!(identity.version, "blue-ts-ecma-lib-v3");
    let mut bytes = format!("{}\0{}\0", identity.version, identity.target).into_bytes();
    for name in &identity.sources {
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(
            &std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src/standard_library")
                    .join(name),
            )
            .unwrap(),
        );
        bytes.push(0);
    }
    let fingerprint = |bytes: &[u8]| {
        let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
        format!(
            "sha256:{}",
            digest
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    };
    assert_eq!(identity.source_fingerprint, fingerprint(&bytes));
    let mut changed_content = bytes.clone();
    changed_content.push(b'\n');
    assert_ne!(identity.source_fingerprint, fingerprint(&changed_content));
    let mut changed_version = bytes;
    changed_version[0] ^= 1;
    assert_ne!(identity.source_fingerprint, fingerprint(&changed_version));
    assert_ne!(
        identity.source_fingerprint,
        standard_library::identity(EcmaTarget::Es5).source_fingerprint
    );
    let modern =
        standard_library::identity_with_libraries(EcmaTarget::Es5, Some(&[EcmaTarget::Es2022]));
    assert_eq!(modern.sources, identity.sources);
    assert_eq!(modern.target, "es5");
    assert_ne!(modern.source_fingerprint, identity.source_fingerprint);
    let empty = standard_library::identity_with_libraries(EcmaTarget::Es2022, Some(&[]));
    assert!(empty.sources.is_empty());
    let owner = |name| CompilerOptions {
        ambient_declaration_modules: vec![ModuleSource::new(
            "memory:///owner.d.ts",
            format!("declare const owned: '{name}';"),
        )],
        ..support::options(&json!({}))
    };
    let source = input["owner_controls"][3]["source"].as_str().unwrap();
    assert_ne!(
        support::checked(source, owner("first")).project_fingerprint,
        support::checked(source, owner("second")).project_fingerprint
    );
}

fn recorder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../development/browser_core/phase-18-bluets/tools/record_gt7_library_manifest.cjs",
    )
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3 and Node"]
fn gt7_native_library_manifest_is_reproducible() {
    let output = Command::new("node").arg(recorder()).output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[ignore = "requires pinned TypeScript 5.9.3, Acorn 8.15.0 and Node"]
fn gt7_exact_library_declarations_have_native_consumers_syntax_and_execution() {
    let mut child = Command::new("node")
        .arg(recorder())
        .arg("--outputs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&support::outputs()).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
