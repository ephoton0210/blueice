// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Per-module checker state shared by binding and expression checks.

use super::*;
use std::cell::Cell;

#[derive(Clone, Copy)]
enum RecordSpreadFailure {
    ResourceLimit,
    UnprovenSource,
}

/// Per-project checking policy shared by every module checker.
#[derive(Clone, Copy)]
pub(super) struct CheckerPolicy {
    pub(super) enforce_types: bool,
    pub(super) require_declared_global_calls: bool,
}

pub(super) struct ModuleChecker<'a> {
    project: &'a Project,
    module: &'a Module,
    exports: &'a ProjectExports,
    ambient: Option<&'a AmbientDeclarations>,
    enforce_types: bool,
    require_declared_global_calls: bool,
    /// Arrow functions already checked, by start offset, so an expression
    /// visited from several checks reports its body once.
    checked_nested_functions: BTreeSet<usize>,
    /// Whether the function being checked is `async`; `None` at module level.
    async_context: Option<bool>,
    /// The readonly fields the running class constructor's own body may still
    /// assign through `this`; `None` outside a constructor and inside any
    /// function nested in one.
    constructor_readonly_fields: Option<BTreeSet<String>>,
    /// The class whose body is being checked, for `private`/`protected` access.
    access_class: Option<String>,
    /// Names of every private or protected class member in scope.
    restricted_member_names: BTreeSet<String>,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) symbols: Vec<Symbol>,
    types: BTreeMap<String, TypeDefinition>,
    values: BTreeMap<String, Type>,
    functions: BTreeMap<String, Vec<FunctionSignature>>,
    class_constructors: BTreeMap<String, ClassConstructorBinding>,
    type_only_classes: BTreeSet<String>,
    function_implementations: BTreeSet<String>,
    type_parameters: BTreeSet<String>,
    allowed_tuple_spread_parameters: BTreeMap<String, Type>,
    max_type_expansions: usize,
    /// Catch bindings are strict `unknown`; the existing inference fallback
    /// uses `Unknown` permissively outside the catch body.
    strict_catch_unknown: bool,
    /// Inference uses `&self`; checked validation emits its first failure.
    record_spread_inference_failure: Cell<Option<(usize, usize, RecordSpreadFailure)>>,
}

mod binding;
mod expressions;
pub(in crate::checker) use binding::{class_export, class_instance_type};
