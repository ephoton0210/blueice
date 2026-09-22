// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stable, source-addressable compiler diagnostics.

use std::fmt;

/// A half-open byte range in a particular source module.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub module: String,
    pub start: usize,
    pub end: usize,
}

impl SourceSpan {
    pub fn new(module: impl Into<String>, start: usize, end: usize) -> Self {
        Self {
            module: module.into(),
            start,
            end,
        }
    }
}

/// A deliberately small stable diagnostic taxonomy.  Consumers should use the
/// code and span, not compiler prose, as their machine interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    ParseError,
    UnsupportedSyntax,
    ModuleNotFound,
    CircularModuleDependency,
    InvalidDeclarationFile,
    DuplicateDeclaration,
    UnknownName,
    UnknownType,
    TypeMismatch,
    ReturnTypeMismatch,
    InvalidContract,
    ResourceLimit,
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::ParseError => "BTS1000",
            Self::UnsupportedSyntax => "BTS1001",
            Self::ModuleNotFound => "BTS2000",
            Self::CircularModuleDependency => "BTS2001",
            Self::InvalidDeclarationFile => "BTS2002",
            Self::DuplicateDeclaration => "BTS3000",
            Self::UnknownName => "BTS3001",
            Self::UnknownType => "BTS3002",
            Self::TypeMismatch => "BTS3003",
            Self::ReturnTypeMismatch => "BTS3004",
            Self::InvalidContract => "BTS4000",
            Self::ResourceLimit => "BTS9000",
        };
        f.write_str(code)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
}

/// A structured compiler report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub span: SourceSpan,
}

impl Diagnostic {
    pub fn error(code: DiagnosticCode, span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            span,
        }
    }
}
