// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structured module imports and exports retain closed graph identities.

use super::SourceSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDeclaration {
    /// `import x = require("m")`: one binding of the module's `export =` value.
    pub equals_require: bool,
    pub type_only: bool,
    pub specifier: String,
    pub specifier_span: SourceSpan,
    pub bindings: Vec<ImportBinding>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    pub imported: String,
    pub local: String,
    pub type_only: bool,
}

/// A static-only `export type` declaration.  It has no JavaScript runtime
/// representation but can still extend the closed type-module graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeExportDeclaration {
    pub bindings: Vec<String>,
    pub specifier: Option<String>,
    pub span: SourceSpan,
}

/// A default export of a local identifier or a retained expression snapshot.
/// Its binding records the value used by checking and declaration emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultExportDeclaration {
    pub name: String,
    /// Expressions use an internal snapshot binding, absent from source text.
    pub expression: bool,
    pub span: SourceSpan,
}

/// A local, named, star or namespace value export. The retained graph edge
/// and binding identities drive checking, runtime linking and declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueExportDeclaration {
    /// `export = name;`: the module's whole export is that value, as the one
    /// binding named `export=`.
    pub export_assignment: bool,
    pub bindings: Vec<ValueExportBinding>,
    /// A retained, closed graph edge for `export ... from "module"`.
    pub specifier: Option<String>,
    pub specifier_span: Option<SourceSpan>,
    /// `export *`, excluding `default`; an optional name denotes `* as name`.
    pub star: bool,
    pub namespace: Option<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueExportBinding {
    pub local: String,
    pub exported: String,
    pub span: SourceSpan,
}
