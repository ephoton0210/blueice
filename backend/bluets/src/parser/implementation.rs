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
    nested_functions: BTreeMap<usize, NestedFunction>,
    diagnostics: Vec<Diagnostic>,
    max_type_depth: usize,
    type_depth: usize,
    /// Set while a class constructor's parameters are parsed, where an
    /// accessibility modifier or `readonly` declares a property.
    parameter_property_mode: bool,
    parameter_properties: Vec<ParameterProperty>,
    /// How many namespace bodies the cursor is inside, and how many of them are
    /// ambient (`declare namespace`).
    namespace_depth: usize,
    ambient_depth: usize,
    /// How many `export {};` markers the body being parsed has had.
    namespace_export_markers: usize,
}

#[path = "declarations.rs"]
mod declarations;
#[path = "runtime_syntax.rs"]
mod runtime_syntax;
#[path = "type_syntax.rs"]
mod type_syntax;
