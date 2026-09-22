// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Private parser state and its focused parsing submodules.

use super::*;

pub(super) struct Parser {
    id: String,
    source: String,
    tokens: Vec<Token>,
    index: usize,
    declarations: Vec<Declaration>,
    edits: Vec<TextEdit>,
    generic_call_type_arguments: BTreeMap<usize, Vec<Type>>,
    diagnostics: Vec<Diagnostic>,
    max_type_depth: usize,
    type_depth: usize,
}

#[path = "declarations.rs"]
mod declarations;
#[path = "runtime_syntax.rs"]
mod runtime_syntax;
#[path = "type_syntax.rs"]
mod type_syntax;
