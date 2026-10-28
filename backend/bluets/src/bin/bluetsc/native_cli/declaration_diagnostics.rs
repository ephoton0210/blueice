// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Physical display names come only from modules admitted by the file owner.
use super::*;

pub(super) fn qualify(diagnostics: &mut [Diagnostic], project: &Project, loader: &FileLoader) {
    for diagnostic in diagnostics {
        let Some(counterpart) = &mut diagnostic.typescript else {
            continue;
        };
        // Rewrite only identities of modules already admitted by this owner.
        // Import type spellings in ordinary diagnostics use the same paths.
        for id in project.modules.keys() {
            let logical = format!(
                "import({})",
                serde_json::to_string(without_extension(id)).expect("module spelling")
            );
            if !counterpart.message.contains(&logical) {
                continue;
            }
            let Ok(path) = loader.source_path(id) else {
                continue;
            };
            let text = path.to_string_lossy().replace('\\', "/");
            let physical = format!(
                "import({})",
                serde_json::to_string(without_extension(&text)).expect("module spelling")
            );
            counterpart.message = counterpart.message.replace(&logical, &physical);
            for argument in &mut counterpart.arguments {
                *argument = argument.replace(&logical, &physical);
            }
        }
        let slot = match counterpart.code {
            4023 => 2,
            4058 => 1,
            _ => continue,
        };
        let Some(argument) = counterpart.arguments.get(slot) else {
            continue;
        };
        let Ok(origin) = serde_json::from_str::<String>(argument) else {
            continue;
        };
        let Some(id) = project
            .modules
            .keys()
            .find(|id| without_extension(id) == origin)
        else {
            continue;
        };
        let Ok(path) = loader.source_path(id) else {
            continue;
        };
        let text = path.to_string_lossy().replace('\\', "/");
        counterpart.arguments[slot] =
            serde_json::to_string(without_extension(&text)).expect("module spelling");
        let mut message = counterpart.message_template.to_string();
        for (index, argument) in counterpart.arguments.iter().enumerate() {
            message = message.replace(&format!("{{{index}}}"), argument);
        }
        counterpart.message = message;
    }
}

fn without_extension(name: &str) -> &str {
    for suffix in [
        ".d.ts", ".d.mts", ".d.cts", ".tsx", ".ts", ".mts", ".cts", ".jsx", ".js", ".mjs", ".cjs",
    ] {
        if let Some(name) = name.strip_suffix(suffix) {
            return name;
        }
    }
    name
}
