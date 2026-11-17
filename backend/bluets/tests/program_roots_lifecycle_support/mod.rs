// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Complete mapped/unmapped lifecycle observations at the public library boundary.

use blueice_bluets::{
    compile_program, CheckingOptions, Compilation, CompilerOptions, EcmaTarget,
    IncrementalCompiler, JavaScriptLayout, ModuleKind,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[path = "../program_roots_support/mod.rs"]
mod owners;
const PREFIX: &str = "program:///";

fn options(profile: &Value) -> CompilerOptions {
    let profile = &profile["compilerOptions"];
    CompilerOptions {
        target: EcmaTarget::parse(profile["target"].as_str().unwrap()).unwrap(),
        module_kind: match profile["module"].as_str().unwrap() {
            "CommonJS" => ModuleKind::CommonJs,
            "ESNext" => ModuleKind::Esm,
            other => panic!("unexpected authored module kind {other}"),
        },
        libraries: Some(vec![EcmaTarget::Es5]),
        checking: Some(CheckingOptions::default()),
        declaration: true,
        source_map: profile["sourceMap"].as_bool().unwrap(),
        declaration_map: profile["declarationMap"].as_bool().unwrap(),
        javascript_layout: JavaScriptLayout::TypeScript,
        always_strict: true,
        ..CompilerOptions::default()
    }
}

fn diagnostic(row: &Value) -> Value {
    let row = row.get("typescript").unwrap_or(row);
    if !row["code"].is_number() {
        return json!({"missingCounterpart":true,"raw":row});
    }
    let module = row["span"]["module"].as_str().unwrap();
    json!({"code":row["code"],"message":row["message"],
        "file":module.strip_prefix(PREFIX).unwrap_or(module),"position":row["position"],
        "relatedInformation":row["relatedInformation"].as_array().into_iter().flatten()
            .map(diagnostic).collect::<Vec<_>>()})
}

fn observation(compilation: &Compilation) -> Value {
    let raw = compilation
        .diagnostics
        .iter()
        .map(|row| row.to_json())
        .collect::<Vec<_>>();
    let mut declarations = BTreeMap::new();
    let mut artifacts = BTreeMap::new();
    if let Some(output) = &compilation.output {
        for (id, artifact) in &output.artifacts {
            let name = id
                .strip_prefix(PREFIX)
                .unwrap()
                .strip_suffix(".ts")
                .unwrap();
            if let Some(text) = &artifact.declaration {
                let mut file = text.clone();
                if artifact.declaration_map.is_some() {
                    let basename = name.rsplit('/').next().unwrap();
                    file.push_str(&format!("//# sourceMappingURL={basename}.d.ts.map"));
                }
                declarations.insert(format!("{name}.d.ts"), file);
            }
            artifacts.insert(
                id,
                json!({"javascript":artifact.javascript,"declarationBody":artifact.declaration,
                    "sourceMap":artifact.source_map.as_ref().map(|map| map.to_json()),
                    "declarationMap":artifact.declaration_map.as_ref().map(|map| map.to_json()),
                    "fingerprint":artifact.fingerprint}),
            );
        }
    }
    json!({"accepts":!compilation.has_errors(),"diagnostics":raw.iter().map(diagnostic).collect::<Vec<_>>(),
        "declarations":declarations,"modules":compilation.project.modules.keys()
            .map(|id|id.strip_prefix(PREFIX).unwrap()).collect::<Vec<_>>(),
        "projectFingerprint":compilation.project_fingerprint,
        "programRoots":compilation.project.program_roots,"rawDiagnostics":raw,
        "artifacts":artifacts,"debugInfo":format!("{:?}",compilation.debug_info),
        "hasOutput":compilation.output.is_some(),"hasDebugInfo":compilation.debug_info.is_some()})
}

fn partition(
    first: &BTreeSet<String>,
    second: &BTreeSet<String>,
    current: &BTreeSet<String>,
) -> bool {
    first.is_disjoint(second) && first.union(second).eq(current.iter())
}

pub fn run(mut record: impl FnMut(Value)) {
    let fixture: Value = serde_json::from_str(include_str!(
        "../fixtures/program_roots/library-lifecycle.json"
    ))
    .unwrap();
    let mut total = 0;
    for sequence in fixture["sequences"].as_array().unwrap() {
        let mut compiler = IncrementalCompiler::new();
        let mut last_success: Option<&Value> = None;
        for state in sequence["states"].as_array().unwrap() {
            if state["clean"] == true {
                compiler = IncrementalCompiler::new();
                last_success = None;
            }
            let roots = state["roots"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| format!("{PREFIX}{}", name.as_str().unwrap()))
                .collect::<Vec<_>>();
            let loader = owners::loader(&state["files"]);
            let options = options(&sequence["profile"]);
            let cold = compile_program(&roots, &loader, options.clone());
            let warm = compiler.compile_program(&roots, &loader, options);
            let cold_observation = observation(&cold);
            let observed = observation(&warm.compilation);
            let mut differences = Vec::new();
            for key in ["accepts", "diagnostics", "declarations", "modules"] {
                if observed[key] != state["expected"][key] {
                    differences.push(key);
                }
            }
            let current = warm.compilation.project.modules.keys().cloned().collect();
            let parsed_partition =
                partition(&warm.parsed_modules, &warm.reused_parsed_modules, &current);
            let checked_partition = partition(
                &warm.rechecked_modules,
                &warm.reused_checked_modules,
                &current,
            );
            let requires_full_cache = last_success.is_some_and(|previous| {
                previous["files"] == state["files"] && previous["roots"] == state["roots"]
            });
            let requires_cold =
                last_success.is_none_or(|previous| previous["roots"] != state["roots"]);
            let cache_contract = (!requires_full_cache || warm.cache_hit)
                && (!requires_cold || !warm.cache_hit && warm.reused_parsed_modules.is_empty())
                && (!warm.cache_hit
                    || warm.parsed_modules.is_empty() && warm.rechecked_modules.is_empty())
                && (!warm.compilation.has_errors()
                    || !warm.cache_hit
                        && warm.compilation.output.is_none()
                        && warm.compilation.debug_info.is_none());
            let exact = cold.project_fingerprint == warm.compilation.project_fingerprint
                && cold.diagnostics == warm.compilation.diagnostics
                && cold.output == warm.compilation.output
                && cold.debug_info == warm.compilation.debug_info;
            record(
                json!({"id":format!("{}/{}",sequence["id"].as_str().unwrap(),state["id"].as_str().unwrap()),
                "expected":state["expected"],"cold":cold_observation,"observed":observed,
                "differences":differences,"nativeAgrees":differences.is_empty(),"coldWarmExact":exact,
                "parsedPartition":parsed_partition,"checkedPartition":checked_partition,"cacheContract":cache_contract,
                "cacheHit":warm.cache_hit,"requiresFullCache":requires_full_cache,"requiresCold":requires_cold,
                "parsed":warm.parsed_modules,"reusedParsed":warm.reused_parsed_modules,
                "rechecked":warm.rechecked_modules,"reusedChecked":warm.reused_checked_modules}),
            );
            if !warm.compilation.has_errors() {
                last_success = Some(state);
            }
            total += 1;
        }
    }
    assert_eq!(total, 528);
}
