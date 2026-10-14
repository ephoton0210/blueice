// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Structured module imports and exports retain closed graph identities.

use super::{Declaration, SourceSpan};
use crate::syntax::Token;

pub(crate) fn has_module_syntax(declarations: &[Declaration]) -> bool {
    declarations.iter().any(|declaration| match declaration {
        Declaration::Import(_)
        | Declaration::TypeExport(_)
        | Declaration::DefaultExport(_)
        | Declaration::ValueExport(_) => true,
        Declaration::Variable(item) => item.exported,
        Declaration::Function(item) => item.exported,
        Declaration::Class(item) => item.exported,
        Declaration::Enum(item) => item.exported,
        Declaration::Interface(item) => item.exported,
        Declaration::TypeAlias(item) => item.exported,
        Declaration::Namespace(item) => item.exported,
        Declaration::Ambient(_) | Declaration::Raw(_) | Declaration::UmdExport(_) => false,
    })
}

/// An erased external module declaration or global augmentation. The body
/// retains each declaration's original source identity; it supplies no
/// executable binding or loading authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbientDeclaration {
    /// `None` denotes `declare global`; a name denotes `declare module`.
    pub specifier: Option<String>,
    pub specifier_span: SourceSpan,
    pub body: Vec<Declaration>,
    /// A bodyless external module supplies an untyped static surface.
    pub shorthand: bool,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportAttributes {
    pub assertion: bool,
    pub entries: Vec<ImportAttribute>,
    pub span: SourceSpan,
}

impl ImportAttributes {
    /// The retained type-only edge's package condition; parsing validates it.
    pub fn resolution_mode(&self) -> Option<crate::package_resolution::ImportMode> {
        self.entries.iter().find_map(|entry| {
            if entry.key != "resolution-mode" {
                return None;
            }
            match super::string_contents(&entry.value)?.as_str() {
                "import" => Some(crate::package_resolution::ImportMode::Import),
                "require" => Some(crate::package_resolution::ImportMode::Require),
                _ => None,
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportAttribute {
    pub key: String,
    pub value: Token,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDeclaration {
    /// `import x = require("m")`: one binding of the module's `export =` value.
    pub equals_require: bool,
    pub type_only: bool,
    pub specifier: String,
    pub specifier_span: SourceSpan,
    pub attributes: Option<ImportAttributes>,
    pub bindings: Vec<ImportBinding>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    pub imported: String,
    pub local: String,
    pub type_only: bool,
    pub span: SourceSpan,
}

/// A static-only `export type` declaration.  It has no JavaScript runtime
/// representation but can still extend the closed type-module graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeExportDeclaration {
    pub bindings: Vec<ValueExportBinding>,
    pub star: bool,
    pub specifier: Option<String>,
    pub specifier_span: Option<SourceSpan>,
    pub attributes: Option<ImportAttributes>,
    pub namespace: Option<String>,
    pub span: SourceSpan,
}

/// A declaration module's static global name (`export as namespace Name`).
/// It does not create an executable namespace or a module-loading grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UmdExportDeclaration {
    pub name: String,
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
    pub attributes: Option<ImportAttributes>,
    /// `export *`, excluding `default`; an optional name denotes `* as name`.
    pub star: bool,
    pub namespace: Option<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueExportBinding {
    pub local: String,
    pub exported: String,
    pub type_only: bool,
    pub span: SourceSpan,
}

impl ImportDeclaration {
    /// Inline type bindings also disappear from the runtime graph.
    pub fn is_type_only(&self) -> bool {
        self.type_only
            || !self.bindings.is_empty() && self.bindings.iter().all(|binding| binding.type_only)
    }
}

impl ValueExportDeclaration {
    /// A list consisting only of inline type exports has no runtime edge.
    pub fn is_type_only(&self) -> bool {
        !self.export_assignment
            && !self.star
            && !self.bindings.is_empty()
            && self.bindings.iter().all(|binding| binding.type_only)
    }
}
