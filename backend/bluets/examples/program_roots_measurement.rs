// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Records every frozen public-library observation, including all differences.

use blueice_bluets::{
    compile_program, AuthorizedModuleLoader, CheckingOptions, Compilation, CompilerOptions,
    EcmaTarget, IncrementalCompiler, ModuleKind,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Seek, Write};

const PREFIX: &str = "program:///";
#[path = "../tests/program_roots_support/mod.rs"]
mod program_roots_support;

fn roots(row: &Value) -> Vec<String> {
    row["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| format!("{PREFIX}{}", name.as_str().unwrap()))
        .collect()
}

fn loader(row: &Value) -> AuthorizedModuleLoader {
    program_roots_support::loader(&row["files"])
}

fn options(profile: &Value) -> CompilerOptions {
    let profile = &profile["compilerOptions"];
    assert_eq!(
        profile["sourceMap"], false,
        "this library fixture selects unmapped profiles"
    );
    assert_eq!(profile["declarationMap"], false);
    CompilerOptions {
        target: EcmaTarget::parse(profile["target"].as_str().unwrap()).unwrap(),
        module_kind: match profile["module"].as_str().unwrap() {
            "CommonJS" => ModuleKind::CommonJs,
            "ESNext" => ModuleKind::Esm,
            other => panic!("unexpected authored module kind {other}"),
        },
        checking: Some(CheckingOptions::default()),
        libraries: Some(vec![EcmaTarget::Es5]),
        declaration: true,
        ..CompilerOptions::default()
    }
}

fn native_diagnostic(row: &Value) -> Value {
    let native = row.get("typescript").unwrap_or(row);
    if !native["code"].is_number() {
        return json!({"missingCounterpart":true,"raw":row});
    }
    let module = native["span"]["module"].as_str().unwrap();
    json!({"code":native["code"],"message":native["message"],
        "file":module.strip_prefix(PREFIX).unwrap_or(module),"position":native["position"],
        "relatedInformation":native["relatedInformation"].as_array().into_iter().flatten()
            .map(native_diagnostic).collect::<Vec<_>>()})
}

fn observation(compilation: &Compilation, expected: &Value) -> Value {
    let raw = compilation
        .diagnostics
        .iter()
        .map(|row| row.to_json())
        .collect::<Vec<_>>();
    let diagnostics = raw.iter().map(native_diagnostic).collect::<Vec<_>>();
    let declarations = compilation
        .output
        .iter()
        .flat_map(|output| &output.artifacts)
        .filter_map(|(module, artifact)| {
            artifact.declaration.as_ref().map(|text| {
                let name = module
                    .strip_prefix(PREFIX)
                    .unwrap()
                    .strip_suffix(".ts")
                    .unwrap();
                (format!("{name}.d.ts"), text.clone())
            })
        })
        .collect::<BTreeMap<_, _>>();
    let semantic = json!({"accepts":!compilation.has_errors(),"diagnostics":diagnostics,"declarations":declarations});
    let mut differences = Vec::new();
    for key in ["accepts", "diagnostics", "declarations"] {
        if semantic[key] != expected[key] {
            differences.push(key);
        }
    }
    if let Some(modules) = expected.get("modules") {
        let mut expected = modules
            .as_array()
            .unwrap()
            .iter()
            .map(|name| format!("{PREFIX}{}", name.as_str().unwrap()))
            .collect::<Vec<_>>();
        expected.sort();
        if compilation
            .project
            .modules
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            != expected
        {
            differences.push("moduleInventory");
        }
    }
    json!({"agrees":differences.is_empty(),"differences":differences,"expected":expected,
        "observed":semantic,"rawDiagnostics":raw,"projectFingerprint":compilation.project_fingerprint,
        "programRoots":compilation.project.program_roots,"modules":compilation.project.modules.keys().collect::<Vec<_>>(),
        "hasOutput":compilation.output.is_some()})
}

fn save(file: &mut std::fs::File, report: &Value) {
    file.rewind().unwrap();
    file.set_len(0).unwrap();
    serde_json::to_writer_pretty(&mut *file, report).unwrap();
    file.write_all(b"\n").unwrap();
    file.flush().unwrap();
}

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    assert_eq!(
        arguments.len(),
        2,
        "expected immutable library fixture JSON and a fresh report filename"
    );
    let input: Value = serde_json::from_slice(&std::fs::read(&arguments[0]).unwrap()).unwrap();
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&arguments[1])
        .unwrap();
    let mut report = json!({"conclusion":"in_progress","inventoryCredit":0,
        "scope":"Complete frozen unmapped public-library subset, 128 primary observations and 264 actual incremental states. Full external 512/528 acceptance remains mandatory.",
        "executable":std::env::current_exe().unwrap(),"primary":[],"lifecycle":[]});
    save(&mut output, &report);
    for row in input["primary"].as_array().unwrap() {
        let result = compile_program(&roots(row), &loader(row), options(&row["profile"]));
        let mut observed = observation(&result, &row["expected"]);
        observed["id"] = row["id"].clone();
        report["primary"].as_array_mut().unwrap().push(observed);
        save(&mut output, &report);
    }
    for sequence in input["lifecycle"].as_array().unwrap() {
        let mut compiler = IncrementalCompiler::new();
        for state in sequence["states"].as_array().unwrap() {
            if state["clean"] == true {
                compiler = IncrementalCompiler::new();
            }
            let options = options(&sequence["profile"]);
            let roots = roots(state);
            let loader = loader(state);
            let cold = compile_program(&roots, &loader, options.clone());
            let warm = compiler.compile_program(&roots, &loader, options);
            let mut observed = observation(&warm.compilation, &state["expected"]);
            observed["id"] = json!(format!(
                "{}/{}",
                sequence["id"].as_str().unwrap(),
                state["id"].as_str().unwrap()
            ));
            observed["cacheHit"] = json!(warm.cache_hit);
            observed["parsed"] = json!(warm.parsed_modules);
            observed["reusedParsed"] = json!(warm.reused_parsed_modules);
            observed["rechecked"] = json!(warm.rechecked_modules);
            observed["reusedChecked"] = json!(warm.reused_checked_modules);
            observed["coldWarmExact"] = json!(
                warm.compilation.project_fingerprint == cold.project_fingerprint
                    && warm.compilation.diagnostics == cold.diagnostics
                    && warm.compilation.output == cold.output
                    && warm.compilation.debug_info == cold.debug_info
            );
            report["lifecycle"].as_array_mut().unwrap().push(observed);
            save(&mut output, &report);
        }
    }
    let primary = report["primary"].as_array().unwrap();
    let lifecycle = report["lifecycle"].as_array().unwrap();
    let counts = json!({"primary":primary.len(),"primaryAgreements":primary.iter().filter(|row| row["agrees"] == true).count(),
        "lifecycle":lifecycle.len(),"lifecycleAgreements":lifecycle.iter().filter(|row| row["agrees"] == true).count(),
        "coldWarmExact":lifecycle.iter().filter(|row| row["coldWarmExact"] == true).count()});
    assert_eq!(primary.len(), 128);
    assert_eq!(lifecycle.len(), 264);
    report["counts"] = counts.clone();
    report["conclusion"] = json!("measured-public-library-subset");
    save(&mut output, &report);
    println!("{counts}");
}
