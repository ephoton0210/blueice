// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Public-boundary G-T7 observations; this adapter never loads external files.
use blueice_bluets::{
    compile, CheckingOptions, CompilerOptions, EcmaTarget, MapLoader, ModuleLoader, ModuleSource,
    SourceSpan,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

struct FrozenGraph(BTreeMap<String, String>);

impl ModuleLoader for FrozenGraph {
    fn load(&self, id: &str) -> Result<ModuleSource, String> {
        self.0
            .get(id.strip_prefix("memory:///").ok_or("unowned module id")?)
            .map(|source| ModuleSource::new(id, source))
            .ok_or_else(|| format!("unowned fixture source {id}"))
    }

    fn resolve(&self, _from: &str, specifier: &str) -> Result<String, String> {
        let name = specifier.strip_prefix("./").ok_or("unowned module edge")?;
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err("fixture edge escapes the frozen graph".into());
        }
        let stem = name.strip_suffix(".js").unwrap_or(name);
        [
            name.to_string(),
            format!("{stem}.ts"),
            format!("{stem}.d.ts"),
        ]
        .into_iter()
        .find(|name| self.0.contains_key(name))
        .map(|name| format!("memory:///{name}"))
        .ok_or_else(|| format!("unowned fixture edge {specifier}"))
    }
}

pub fn records() -> (Value, Value) {
    (
        serde_json::from_str(include_str!("../fixtures/gt7_library_manifest/cases.json")).unwrap(),
        serde_json::from_str(include_str!(
            "../fixtures/gt7_library_manifest/reference.json"
        ))
        .unwrap(),
    )
}

pub fn edition(value: &str) -> EcmaTarget {
    match value.to_ascii_lowercase().as_str() {
        "es5" => EcmaTarget::Es5,
        "es2015" => EcmaTarget::Es2015,
        "es2016" => EcmaTarget::Es2016,
        "es2017" => EcmaTarget::Es2017,
        "es2018" => EcmaTarget::Es2018,
        "es2019" => EcmaTarget::Es2019,
        "es2020" => EcmaTarget::Es2020,
        "es2021" => EcmaTarget::Es2021,
        "es2022" => EcmaTarget::Es2022,
        "es2023" => EcmaTarget::Es2023,
        other => panic!("unlisted native edition {other}"),
    }
}

pub fn options(row: &Value) -> CompilerOptions {
    let value = &row["options"];
    CompilerOptions {
        target: edition(value["target"].as_str().unwrap_or("ES2022")),
        libraries: Some(
            value["lib"]
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .map(|v| edition(v.as_str().unwrap()))
                        .collect()
                })
                .unwrap_or_else(|| vec![EcmaTarget::Es2022]),
        ),
        declaration: true,
        checking: Some(CheckingOptions::default()),
        ..CompilerOptions::default()
    }
}

fn position(
    code: u32,
    message: &str,
    span: &SourceSpan,
    files: &BTreeMap<String, String>,
) -> Value {
    let name = span
        .module
        .strip_prefix("memory:///")
        .or_else(|| span.module.strip_prefix("<typescript-lib>/"))
        .unwrap_or(&span.module);
    let (start, length) = if let Some(source) = files.get(name) {
        (
            source[..span.start].encode_utf16().count(),
            source[span.start..span.end].encode_utf16().count(),
        )
    } else {
        // Pinned public library locations are already native UTF-16 offsets;
        // they require neither an owner loader read nor a local npm path.
        (span.start, span.end - span.start)
    };
    json!({"code": code, "message": message, "module": name, "start": start, "length": length, "related": []})
}

fn diagnostics(result: &blueice_bluets::Compilation, files: &BTreeMap<String, String>) -> Value {
    json!(result
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let Some(native) = &diagnostic.typescript else {
                return diagnostic.to_json();
            };
            let mut value = position(native.code, &native.message, &native.span, files);
            value["related"] = json!(native
                .related_information
                .iter()
                .map(|related| position(related.code, &related.message, &related.span, files))
                .collect::<Vec<_>>());
            value
        })
        .collect::<Vec<_>>())
}

pub fn checked(source: &str, options: CompilerOptions) -> blueice_bluets::Compilation {
    compile(
        "memory:///entry.ts",
        &MapLoader::from([ModuleSource::new("memory:///entry.ts", source)]),
        options,
    )
}

pub fn checked_files(
    entry: &str,
    files: &[(&str, &str)],
    options: CompilerOptions,
) -> blueice_bluets::Compilation {
    let loader = FrozenGraph(
        files
            .iter()
            .map(|(name, source)| ((*name).into(), (*source).into()))
            .collect(),
    );
    compile(&format!("memory:///{entry}"), &loader, options)
}

pub fn outputs() -> Vec<Value> {
    let (_, reference) = records();
    let mut failures = Vec::new();
    let mut outputs = Vec::new();
    for row in reference["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(reference["profile_controls"].as_array().unwrap())
    {
        let files: BTreeMap<String, String> = row["files"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, source)| (name.clone(), source.as_str().unwrap().into()))
            .collect();
        let loader = FrozenGraph(files.clone());
        let result = compile("memory:///entry.ts", &loader, options(row));
        let actual = diagnostics(&result, &files);
        if actual != row["diagnostics"]
            || result.output.is_some() != row["accepts"].as_bool().unwrap()
        {
            failures.push(format!(
                "{}: expected {}; got {}; output={}",
                row["id"],
                row["diagnostics"],
                actual,
                result.output.is_some()
            ));
            continue;
        }
        let Some(output) = result.output else {
            continue;
        };
        let mut declarations = BTreeMap::new();
        let mut javascript = BTreeMap::new();
        for (name, artifact) in output.artifacts {
            let stem = name
                .strip_prefix("memory:///")
                .unwrap()
                .strip_suffix(".ts")
                .unwrap();
            if let Some(declaration) = artifact.declaration {
                declarations.insert(format!("{stem}.d.ts"), declaration);
            }
            javascript.insert(format!("{stem}.js"), artifact.javascript);
        }
        if json!(declarations) != row["declarations"] {
            failures.push(format!(
                "{}: declarations expected {}; got {}",
                row["id"],
                row["declarations"],
                json!(declarations)
            ));
        }
        for (kind, consumer) in row["consumers"].as_object().into_iter().flatten() {
            let mut files = declarations.clone();
            files.insert(
                "consumer.ts".into(),
                consumer["source"].as_str().unwrap().into(),
            );
            let loader = FrozenGraph(files.clone());
            let result = compile("memory:///consumer.ts", &loader, options(row));
            let actual = diagnostics(&result, &files);
            if actual != consumer["diagnostics"] {
                failures.push(format!(
                    "{} consumer {kind}: expected {}; got {}",
                    row["id"], consumer["diagnostics"], actual
                ));
            }
        }
        outputs
            .push(json!({"id": row["id"], "declarations": declarations, "javascript": javascript}));
    }
    assert!(
        failures.is_empty(),
        "{} G-T7 mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
    outputs
}
