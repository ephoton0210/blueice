// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The standalone, host-neutral BlueTS front end.
//!
//! This crate deliberately has no dependency on BlueJS, a DOM crate, or an
//! I/O crate.  A host supplies a closed set of already-authorized module
//! records to [`compile`]; BlueTS parses, binds, checks, lowers, and emits
//! TypeScript-derived ECMAScript without acquiring files, URLs, capabilities,
//! or evaluation authority of its own.
//!
//! The initial language matrix is intentionally small and explicit.  It
//! supports typed variables and functions, interfaces, type aliases, type-only
//! imports and exports, and the primitive/record/array/tuple/union type forms
//! exercised by the public tests.  Unsupported TypeScript syntax is diagnosed
//! as `UnsupportedSyntax`; it is never passed through accidentally as
//! JavaScript.

mod checker;
mod compiler;
mod contracts;
mod debug_info;
mod diagnostic;
mod emitter;
mod parser;
mod syntax;

pub use checker::{CheckedModule, CheckedProject, Symbol, SymbolKind, Type};
pub use compiler::{
    compile, CompilerLimits, CompilerOptions, EcmaTarget, IncrementalCompiler, IncrementalResult,
    MapLoader, ModuleLoader, ModuleSource, Project, RuntimePolicy,
};
pub use contracts::{
    Contract, ContractError, ContractPlan, ContractValue, ValidationError, ValidationLimits,
};
pub use debug_info::{BlueTsDebugInfo, DebugSource, DebugSymbol, DebugType, SymbolId, TypeId};
pub use diagnostic::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
pub use emitter::{BuildArtifact, BuildOutput, SourceMap};
pub use parser::{
    Declaration, FunctionDeclaration, ImportDeclaration, InterfaceDeclaration, Module, Parameter,
    ParserLimits, TypeAliasDeclaration, TypeExportDeclaration, TypeParameter, VariableDeclaration,
};

/// The pinned BlueTS language matrix exposed in emitted fingerprints and
/// diagnostics.  This is not a claim of complete `tsc` compatibility.
pub const LANGUAGE_VERSION: &str = "blue-ts-0.1";

/// A successful compilation, including checked modules and optional portable
/// JavaScript artifacts.  Diagnostics are always returned; callers must only
/// publish artifacts when [`Self::has_errors`] is false.
#[derive(Debug, Clone)]
pub struct Compilation {
    pub project: Project,
    pub checked: Option<CheckedProject>,
    pub debug_info: Option<BlueTsDebugInfo>,
    pub diagnostics: Vec<Diagnostic>,
    pub output: Option<BuildOutput>,
}

impl Compilation {
    /// Returns true when parsing, resolution, binding, checking, lowering, or
    /// emission found an error.  The compiler never creates `output` in this
    /// state.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }
}
