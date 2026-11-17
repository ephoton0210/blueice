// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixture owners supply exact Native-calibrated edges; the compiler guesses none.

use blueice_bluets::{AuthorizedModule, AuthorizedModuleLoader, AuthorizedModuleResolution};
use serde_json::Value;
use std::sync::OnceLock;

pub fn loader(files: &Value) -> AuthorizedModuleLoader {
    static OWNERS: OnceLock<Value> = OnceLock::new();
    let owners = OWNERS.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../fixtures/program_roots/owner-resolutions.json"
        ))
        .unwrap()
    });
    let edges = owners["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["files"] == *files)
        .and_then(|group| group["edges"].as_array())
        .into_iter()
        .flatten()
        .map(|edge| {
            AuthorizedModuleResolution::new(
                format!("program:///{}", edge["from"].as_str().unwrap()),
                edge["specifier"].as_str().unwrap(),
                format!("program:///{}", edge["target"].as_str().unwrap()),
            )
        });
    let modules = files.as_object().unwrap().iter().map(|(name, text)| {
        AuthorizedModule::new(format!("program:///{name}"), text.as_str().unwrap())
    });
    AuthorizedModuleLoader::new(modules, edges).unwrap()
}
