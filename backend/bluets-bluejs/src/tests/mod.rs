// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shared fixtures for direct BlueTS-to-BlueJS lowering tests.

use super::*;
use blueice_bluets::{MapLoader, ModuleSource};

pub(super) const ENTRY: &str = "memory:///direct.ts";
pub(super) const MODULE_ENTRY: &str = "memory:///direct-module.ts";
pub(super) const GRAPH_ENTRY: &str = "graph/main.ts";

pub(super) fn expression_tokens(parts: &[(&str, TokenKind)]) -> Vec<Token> {
    let mut start = 0;
    parts
        .iter()
        .map(|(text, kind)| {
            let end = start + text.len();
            let token = Token {
                kind: *kind,
                text: (*text).to_string(),
                start,
                end,
            };
            start = end + 1;
            token
        })
        .collect()
}

pub(super) struct AliasedGraphLoader;

impl ModuleLoader for AliasedGraphLoader {
    fn load(&self, module_id: &str) -> Result<ModuleSource, String> {
        match module_id {
            "virtual/main.ts" => Ok(ModuleSource::new(
                module_id,
                "import { value } from '@runtime'; \
                     export const answer: number = value + 1; answer;",
            )),
            "canonical/runtime.ts" => Ok(ModuleSource::new(
                module_id,
                "export const value: number = 41;",
            )),
            _ => Err(format!("unexpected module request `{module_id}`")),
        }
    }

    fn resolve(&self, _from_module: &str, specifier: &str) -> Result<String, String> {
        match specifier {
            "@runtime" => Ok("canonical/runtime.ts".to_string()),
            _ => Err(format!("unexpected import specifier `{specifier}`")),
        }
    }
}

mod debug_attachment;
mod direct;
mod expressions;
mod page_runtime;
