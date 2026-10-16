// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Suite inventory only. Test-function counts are not program pass rates.

use super::rust_facts;
use super::snapshot::{relative, Result, Snapshot};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const DIRECTORY: &str = "development/browser_core/phase-18-bluets";
pub const ARTIFACT: &str = "COMPATIBILITY_SUITES.json";
pub const START: &str = "<!-- BEGIN GENERATED K14 SUITES -->";
pub const END: &str = "<!-- END GENERATED K14 SUITES -->";

#[derive(Debug, Serialize)]
pub struct Inventory {
    pub schema: u32,
    pub typescript: &'static str,
    pub source_revision: String,
    pub source_graph_sha256: String,
    pub fixture_directories: usize,
    pub diagnostic_programs: usize,
    pub diagnostic_templates: usize,
    pub diagnostic_accepts: usize,
    pub diagnostic_rejects: usize,
    pub suites: Vec<Suite>,
}

#[derive(Debug, Serialize)]
pub struct Suite {
    pub harness: String,
    pub ignored_tests: Vec<String>,
    /// Git blob ids make the module/include closure independently auditable.
    pub inputs: BTreeMap<String, String>,
}

pub fn generate(snapshot: &Snapshot) -> Result<Inventory> {
    let mut suites = Vec::new();
    for path in snapshot.blobs.keys().filter(|path| {
        ["backend/bluets/tests/", "backend/bluets-bluejs/tests/"]
            .iter()
            .any(|root| {
                path.strip_prefix(root)
                    .is_some_and(|name| name.ends_with(".rs") && !name.contains('/'))
            })
    }) {
        let mut suite = Suite {
            harness: path.clone(),
            ignored_tests: Vec::new(),
            inputs: BTreeMap::new(),
        };
        let (parent, _) = path.rsplit_once('/').ok_or("invalid harness path")?;
        collect(snapshot, path, parent, "", &mut BTreeSet::new(), &mut suite)?;
        if !suite.ignored_tests.is_empty() {
            suite.ignored_tests.sort();
            let unique: BTreeSet<_> = suite.ignored_tests.iter().collect();
            if unique.len() != suite.ignored_tests.len() {
                return Err(format!("duplicate ignored test in {path}").into());
            }
            suites.push(suite);
        }
    }
    if suites.is_empty() {
        return Err("snapshot contains no oracle suites".into());
    }
    let mut graph = String::new();
    for (path, blob) in &snapshot.blobs {
        graph.push_str(&format!("{blob}\t{path}\n"));
    }
    let digest = ring::digest::digest(&ring::digest::SHA256, graph.as_bytes());
    let source_graph_sha256 = digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let corpus = "backend/bluets/tests/fixtures/typescript_oracle/";
    let fixture_directories = snapshot
        .blobs
        .keys()
        .filter_map(|path| {
            path.strip_prefix(corpus)
                .and_then(|path| path.split_once('/').map(|(name, _)| name))
        })
        .collect::<BTreeSet<_>>()
        .len();
    let diagnostics: Value = serde_json::from_str(
        snapshot.source("backend/bluets/tests/fixtures/diagnostics/reference.json")?,
    )?;
    if diagnostics["version"] != "5.9.3" {
        return Err("unpinned diagnostics version".into());
    }
    let cases = diagnostics["cases"]
        .as_array()
        .ok_or("missing diagnostic cases")?;
    let mut ids = BTreeSet::new();
    let mut accepts = 0;
    let mut rejects = 0;
    for case in cases {
        let id = case["id"].as_str().ok_or("missing diagnostic case id")?;
        if !ids.insert(id) {
            return Err(format!("duplicate diagnostic case: {id}").into());
        }
        match case["verdict"].as_str() {
            Some("accept") => accepts += 1,
            Some("reject") => rejects += 1,
            _ => return Err(format!("unknown diagnostic verdict: {id}").into()),
        }
    }
    Ok(Inventory {
        schema: 1,
        typescript: "5.9.3",
        source_revision: snapshot.revision.clone(),
        source_graph_sha256,
        fixture_directories,
        diagnostic_programs: cases.len(),
        diagnostic_templates: diagnostics["templates"]
            .as_array()
            .ok_or("missing templates")?
            .len(),
        diagnostic_accepts: accepts,
        diagnostic_rejects: rejects,
        suites,
    })
}

fn collect(
    snapshot: &Snapshot,
    path: &str,
    module_directory: &str,
    prefix: &str,
    stack: &mut BTreeSet<String>,
    suite: &mut Suite,
) -> Result<()> {
    if !stack.insert(path.to_string()) {
        return Err(format!("module cycle: {path}").into());
    }
    let facts = rust_facts::parse(snapshot.source(path)?)?;
    let (parent, _) = path.rsplit_once('/').ok_or("invalid source path")?;
    suite
        .inputs
        .insert(path.to_string(), snapshot.blobs[path].clone());
    for name in facts.ignored_tests {
        suite.ignored_tests.push(join_scope(prefix, &name));
    }
    for include in facts.includes {
        let input = relative(parent, &include)?;
        let blob = snapshot
            .blobs
            .get(&input)
            .ok_or_else(|| format!("untracked include: {path} -> {input}"))?;
        suite.inputs.insert(input, blob.clone());
    }
    for module in facts.modules {
        let scope = module.scope.replace("::", "/");
        let logical = join_scope(prefix, &join_scope(&module.scope, &module.name));
        let child = if let Some(custom) = module.path {
            relative(&relative(parent, &scope)?, &custom)?
        } else {
            let base = relative(module_directory, &scope)?;
            let direct = relative(&base, &format!("{}.rs", module.name))?;
            let nested = relative(&base, &format!("{}/mod.rs", module.name))?;
            match (
                snapshot.blobs.contains_key(&direct),
                snapshot.blobs.contains_key(&nested),
            ) {
                (true, false) => direct,
                (false, true) => nested,
                _ => return Err(format!("missing or ambiguous module: {path} -> {logical}").into()),
            }
        };
        let child_directory = child
            .strip_suffix("/mod.rs")
            .map(String::from)
            .or_else(|| child.strip_suffix(".rs").map(String::from))
            .ok_or("non-Rust module path")?;
        collect(snapshot, &child, &child_directory, &logical, stack, suite)?;
    }
    stack.remove(path);
    Ok(())
}

fn join_scope(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}::{name}")
    }
}

pub fn markdown(inventory: &Inventory) -> String {
    let ignored: usize = inventory
        .suites
        .iter()
        .map(|suite| suite.ignored_tests.len())
        .sum();
    let mut result = format!(
        "{START}\n\nGenerated by `cargo run -p blueice-bluets --example compatibility_inventory -- --write {revision}`.\n\n\
         Source commit: `{revision}`. Pinned compiler: TypeScript **{version}**.\n\
         Input graph SHA-256: `{digest}`.\n\n\
         **{suites} Cargo integration suites, {ignored} ignored Rust test functions.**\n\
         These are harness counts, not program/configuration denominators or a pass rate.\n\
         Unix-only tests remain included in this source inventory; platform selection is separate.\n\
         The complete function names and Git blob ids of each harness's literal include/module closure\n\
         are versioned in [COMPATIBILITY_SUITES.json](COMPATIBILITY_SUITES.json).\n\n\
         | Harness (repository-relative) | Ignored Rust functions |\n\
         | --- | ---: |\n",
        revision=inventory.source_revision, version=inventory.typescript,
        digest=inventory.source_graph_sha256, suites=inventory.suites.len()
    );
    for suite in &inventory.suites {
        result.push_str(&format!(
            "| `{}` | {} |\n",
            suite.harness,
            suite.ignored_tests.len()
        ));
    }
    result.push_str(&format!(
        "\nThe versioned `typescript_oracle/` corpus has **{} top-level fixture directories**.\n\
         The shared diagnostic record contains **{} unique programs, {} templates, {} accepts and {} rejects**.\n\
         These overlapping observations must not be added to the harness count or to each other.\n\n{END}",
        inventory.fixture_directories, inventory.diagnostic_programs, inventory.diagnostic_templates,
        inventory.diagnostic_accepts, inventory.diagnostic_rejects
    ));
    result
}

pub fn replace_section(document: &str, replacement: &str) -> Result<String> {
    let (start, end) = if let (Some(start), Some(end)) = (document.find(START), document.find(END))
    {
        if start >= end
            || document.matches(START).count() != 1
            || document.matches(END).count() != 1
        {
            return Err("invalid generated section markers".into());
        }
        (start, end + END.len())
    } else if !document.contains(START) && !document.contains(END) {
        let heading = "## 1. What is compared with `tsc`, and how\n";
        let start = document.find(heading).ok_or("missing suite heading")? + heading.len();
        let end = document[start..]
            .find("## 2.")
            .ok_or("missing subset heading")?
            + start;
        return Ok(format!(
            "{}\n{}\n\n{}",
            &document[..start],
            replacement,
            &document[end..]
        ));
    } else {
        return Err("incomplete generated section markers".into());
    };
    Ok(format!(
        "{}{}{}",
        &document[..start],
        replacement,
        &document[end..]
    ))
}
