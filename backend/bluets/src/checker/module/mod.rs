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
#[derive(Clone)]
pub(crate) struct CheckerPolicy {
    pub(crate) target: crate::compiler::EcmaTarget,
    pub(crate) enforce_types: bool,
    pub(crate) require_declared_global_calls: bool,
    /// Class fields are defined, not assigned, so a derived redeclaration
    /// without an initializer overwrites the base's value.
    pub(crate) define_class_fields: bool,
    /// Modules are emitted one at a time, so an ambient `const enum` cannot be
    /// used.
    pub(crate) isolated_modules: bool,
    /// The module system of the emitted JavaScript, and whether a default import
    /// of an `export =` module is allowed (`esModuleInterop`).
    pub(crate) module_kind: crate::compiler::ModuleKind,
    pub(crate) es_module_interop: bool,
    pub(crate) jsx: Option<crate::compiler::JsxMode>,
    pub(crate) experimental_decorators: bool,
    pub(crate) jsx_factory: Option<String>,
    pub(crate) jsx_fragment_factory: Option<String>,
}

pub(super) struct ModuleChecker<'a> {
    target: crate::compiler::EcmaTarget,
    project: &'a Project,
    scopes: Option<scopes::ScopeModel<'a>>,
    module: &'a Module,
    exports: &'a ProjectExports,
    ambient: Option<&'a AmbientDeclarations>,
    namespace_exports: &'a NamespaceExports,
    pending_imports: BTreeSet<String>,
    module_namespace_imports: BTreeSet<String>,
    enforce_types: bool,
    require_declared_global_calls: bool,
    define_class_fields: bool,
    isolated_modules: bool,
    module_kind: crate::compiler::ModuleKind,
    es_module_interop: bool,
    jsx_mode: Option<crate::compiler::JsxMode>,
    experimental_decorators: bool,
    jsx_factory: Option<String>,
    jsx_fragment_factory: Option<String>,
    jsx_pragmas: crate::jsx::Pragmas,
    /// JSX elements already checked, by start offset.
    checked_jsx_elements: BTreeSet<usize>,
    /// Arrow functions already checked, by start offset, so an expression
    /// visited from several checks reports its body once.
    checked_nested_functions: BTreeSet<usize>,
    /// Whether the function being checked is `async`; `None` at module level.
    async_context: Option<bool>,
    /// What the generator being checked may yield, return and receive.
    generator_context: Option<binding::GeneratorContext>,
    /// The readonly fields the running class constructor's own body may still
    /// assign through `this`; `None` outside a constructor and inside any
    /// function nested in one.
    constructor_readonly_fields: Option<BTreeSet<String>>,
    /// The class whose body is being checked, for `private`/`protected` access.
    access_class: Option<String>,
    /// The members of each enum bound so far, for merged declarations.
    enum_members: BTreeMap<String, Vec<crate::enum_eval::EvaluatedMember>>,
    /// The local names of the `const enum`s in scope, and those among them that
    /// are ambient.
    const_enums: BTreeSet<String>,
    type_only_enums: BTreeSet<String>,
    ambient_const_enums: BTreeSet<String>,
    /// Every enum declaration of the module, evaluated, in source order.
    enum_evaluations: Vec<crate::enum_eval::EvaluatedEnum>,
    /// Names of every private or protected class member in scope.
    restricted_member_names: BTreeSet<String>,
    /// The path of the namespace whose body this checker binds and checks, or
    /// empty for a module; and what every namespace seen so far exports.
    namespace_path: String,
    namespaces: BTreeMap<String, binding::NamespaceMembers>,
    /// Local names of namespaces imported with `import type`, which have no
    /// value to read.
    type_only_namespaces: BTreeSet<String>,
    /// The variables in scope that were declared with a type annotation, whose
    /// type an assignment is checked against.
    annotated_names: BTreeSet<String>,
    pub(super) diagnostics: Vec<Diagnostic>,
    pub(super) symbols: Vec<Symbol>,
    types: BTreeMap<String, TypeDefinition>,
    values: BTreeMap<String, Type>,
    /// Library bindings are defaults; source and owner bindings retain priority.
    library_values: BTreeSet<String>,
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
mod decorators;
mod expressions;
mod jsx;
pub(in crate::checker) use binding::{class_export, class_instance_type};
pub(crate) use binding::{ExportedValue, NamespaceExport};
