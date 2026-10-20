// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Computed entity names cannot collide with ordinary string property names.
pub(crate) const VERSION: &str = "bluets-computed-compositions/2";
const PREFIX: &str = "\0bluets:computed:";

pub(crate) fn name(module: &str, entity: &str) -> String {
    format!(
        "{PREFIX}{}:{entity}",
        serde_json::to_string(module).unwrap()
    )
}

pub(crate) fn entity(name: &str) -> Option<&str> {
    let rest = name.strip_prefix(PREFIX)?;
    // The JSON module is followed by a colon outside its closing quote.
    rest.rsplit_once("\":").map(|(_, entity)| entity)
}

pub(crate) fn display(name: &str) -> Option<String> {
    entity(name).map(|entity| format!("[{entity}]"))
}
