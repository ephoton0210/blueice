// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-module checker state shared by binding and expression checks.

use super::*;

pub(super) struct ModuleChecker<'a> {
    project: &'a Project,
    module: &'a Module,
    exported_types: &'a BTreeMap<String, BTreeMap<String, TypeDefinition>>,
    ambient: Option<&'a AmbientDeclarations>,
    enforce_types: bool,
    require_declared_global_calls: bool,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) symbols: Vec<Symbol>,
    types: BTreeMap<String, TypeDefinition>,
    values: BTreeMap<String, Type>,
    functions: BTreeMap<String, Vec<FunctionSignature>>,
    function_implementations: BTreeSet<String>,
    type_parameters: BTreeSet<String>,
    max_type_expansions: usize,
}

mod binding;
mod expressions;
