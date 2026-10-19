// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regenerate/check the suite inventory from one immutable source revision.

#[path = "../tests/fixtures/compatibility_inventory/accounting.rs"]
mod accounting;
#[path = "../tests/fixtures/compatibility_inventory/case_inline.rs"]
mod case_inline;
#[path = "../tests/fixtures/compatibility_inventory/case_registry.rs"]
mod case_registry;
#[path = "../tests/fixtures/compatibility_inventory/case_report.rs"]
mod case_report;
#[path = "../tests/fixtures/compatibility_inventory/case_sources.rs"]
mod case_sources;
#[path = "../tests/fixtures/compatibility_inventory/case_supplement.rs"]
mod case_supplement;
#[path = "../tests/fixtures/compatibility_inventory/inventory.rs"]
mod inventory;
#[path = "../tests/fixtures/compatibility_inventory/rust_facts.rs"]
mod rust_facts;
#[path = "../tests/fixtures/compatibility_inventory/snapshot.rs"]
mod snapshot;

fn main() -> snapshot::Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let directory = root.join(inventory::DIRECTORY);
    let artifact = directory.join(inventory::ARTIFACT);
    let document = directory.join("COMPATIBILITY_INVENTORY.md");
    match arguments.first().map(String::as_str) {
        Some("--write-cases") if arguments.len() == 3 => {
            let tree = snapshot::Snapshot::read(&root, &arguments[1])?;
            let suites = inventory::generate(&tree)?;
            let measurement = std::fs::read(&arguments[2])?;
            let report = case_report::generate(&tree, &suites, &measurement)?;
            let original = std::fs::read_to_string(&document)?;
            let updated = case_report::replace_section(&original, &case_report::markdown(&report))?;
            std::fs::write(directory.join(case_report::ARTIFACT), serde_json::to_string_pretty(&report)? + "\n")?;
            std::fs::write(directory.join(case_report::MEASUREMENT), measurement)?;
            std::fs::write(document, updated)?;
            println!("Regenerated {} cases in {} measured suites", report.counts.listed, report.suites.len());
        }
        Some("--check-cases") if arguments.len() == 1 => {
            let recorded: serde_json::Value = serde_json::from_slice(&std::fs::read(directory.join(case_report::ARTIFACT))?)?;
            let measurement = std::fs::read(directory.join(case_report::MEASUREMENT))?;
            let tree = snapshot::Snapshot::read(&root, "HEAD")?;
            let mut suites = inventory::generate(&tree)?;
            if recorded["source_graph_sha256"] != suites.source_graph_sha256 {
                return Err("case input graph differs from the measured tree".into());
            }
            suites.source_revision = recorded["source_revision"].as_str().filter(|revision|
                revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .ok_or("invalid recorded case revision")?.to_string();
            let report = case_report::generate(&tree, &suites, &measurement)?;
            if serde_json::to_value(&report)? != recorded {
                return Err("case inventory differs from its source or measurement".into());
            }
            let original = std::fs::read_to_string(document)?;
            if case_report::replace_section(&original, &case_report::markdown(&report))? != original {
                return Err("generated case Markdown differs from the recorded inventory".into());
            }
            println!("Case inventory and measurement agree ({} cases, {} suites)", report.counts.listed, report.suites.len());
        }
        Some("--cases-audit") if arguments.len() == 2 => {
            let tree = snapshot::Snapshot::read(&root, &arguments[1])?;
            let suites = inventory::generate(&tree)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&case_registry::audit(&tree, &suites.suites)?)?
            );
        }
        Some("--write") if arguments.len() == 2 => {
            let tree = snapshot::Snapshot::read(&root, &arguments[1])?;
            let measured = inventory::generate(&tree)?;
            let report = std::fs::read(directory.join(case_report::MEASUREMENT)).ok()
                .and_then(|bytes| case_report::generate(&tree, &measured, &bytes).ok());
            let markdown = inventory::markdown(&measured, report.as_ref());
            let updated =
                inventory::replace_section(&std::fs::read_to_string(&document)?, &markdown)?;
            std::fs::write(artifact, serde_json::to_string_pretty(&measured)? + "\n")?;
            std::fs::write(document, updated)?;
            println!(
                "Regenerated {} suites at {}",
                measured.suites.len(),
                measured.source_revision
            );
        }
        Some("--check") if arguments.len() == 1 => {
            let recorded: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(artifact)?)?;
            // A shallow CI checkout may not contain the measured parent commit.
            // Compare every relevant tree blob to HEAD via the graph digest,
            // then retain the original evidence revision in the rendered text.
            let tree = snapshot::Snapshot::read(&root, "HEAD")?;
            let mut measured = inventory::generate(&tree)?;
            measured.source_revision = recorded["source_revision"]
                .as_str()
                .filter(|revision| {
                    revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit())
                })
                .ok_or("invalid recorded source revision")?
                .to_string();
            if serde_json::to_value(&measured)? != recorded {
                return Err("suite inventory is stale; regenerate at a committed revision".into());
            }
            let original = std::fs::read_to_string(document)?;
            let report = std::fs::read(directory.join(case_report::MEASUREMENT)).ok()
                .and_then(|bytes| case_report::generate(&tree, &measured, &bytes).ok());
            if inventory::replace_section(&original, &inventory::markdown(&measured, report.as_ref()))? != original {
                return Err("generated Markdown differs from the recorded inventory".into());
            }
            println!(
                "Suite inventory and input graph agree ({} suites)",
                measured.suites.len()
            );
        }
        _ => return Err(
            "usage: compatibility_inventory --write <commit> | --check | --cases-audit <commit> | --write-cases <commit> <measurement.json> | --check-cases"
                .into(),
        ),
    }
    Ok(())
}
