// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Regenerate/check the suite inventory from one immutable source revision.

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
        Some("--write") if arguments.len() == 2 => {
            let tree = snapshot::Snapshot::read(&root, &arguments[1])?;
            let measured = inventory::generate(&tree)?;
            let markdown = inventory::markdown(&measured);
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
            if inventory::replace_section(&original, &inventory::markdown(&measured))? != original {
                return Err("generated Markdown differs from the recorded inventory".into());
            }
            println!(
                "Suite inventory and input graph agree ({} suites)",
                measured.suites.len()
            );
        }
        _ => return Err("usage: compatibility_inventory --write <commit> | --check".into()),
    }
    Ok(())
}
