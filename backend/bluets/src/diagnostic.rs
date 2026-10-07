// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stable, source-addressable compiler diagnostics.

use std::fmt;

mod blue_only_rules;
mod mapping;
mod rules;
mod source_rules;
pub(crate) mod spelling;
pub(crate) mod templates;

/// Diagnostic compatibility data is part of compiler/cache identity.
pub const DIAGNOSTICS_VERSION: &str = "typescript-5.9.3-diagnostics-v1";

/// A TypeScript diagnostic counterpart, separate from the stable BTS alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeScriptDiagnostic {
    pub code: u32,
    pub message_template: &'static str,
    pub arguments: Vec<String>,
    pub message: String,
    pub span: SourceSpan,
}

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
    UsedBeforeDeclaration,
    ImmutableAssignment,
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
            Self::UsedBeforeDeclaration => "BTS3005",
            Self::ImmutableAssignment => "BTS3006",
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
    pub typescript: Option<Box<TypeScriptDiagnostic>>,
    pub no_typescript_counterpart: Option<&'static str>,
}

impl Diagnostic {
    /// Attach semantic details at a checker site instead of inferring them
    /// from a broad BTS category or a user-controlled source identity.
    pub(crate) fn with_typescript(mut self, code: u32, arguments: Vec<String>) -> Self {
        self.typescript = mapping::build(code, &self.span, arguments, &self.message).map(Box::new);
        self.no_typescript_counterpart = None;
        self
    }

    pub(crate) fn blue_only(mut self, reason: &'static str) -> Self {
        self.typescript = None;
        self.no_typescript_counterpart = Some(reason);
        self
    }

    pub fn error(code: DiagnosticCode, span: SourceSpan, message: impl Into<String>) -> Self {
        let message = message.into();
        let typescript = mapping::map(code, &span, &message).map(Box::new);
        let no_typescript_counterpart = mapping::no_counterpart(code, &message);
        Self {
            code,
            severity: Severity::Error,
            message,
            span,
            typescript,
            no_typescript_counterpart,
        }
    }

    /// An opt-in machine representation preserves both diagnostic identities.
    /// Source text is never included, and this method performs no I/O.
    pub fn to_json(&self) -> serde_json::Value {
        let counterpart = self.typescript.as_ref().map(|diagnostic| {
            serde_json::json!({
                "code": diagnostic.code,
                "messageTemplate": diagnostic.message_template,
                "arguments": diagnostic.arguments,
                "message": diagnostic.message,
                "span": {
                    "module": diagnostic.span.module,
                    "start": diagnostic.span.start,
                    "end": diagnostic.span.end,
                },
            })
        });
        serde_json::json!({
            "btsCode": self.code.to_string(),
            "severity": match self.severity { Severity::Error => "error", Severity::Warning => "warning" },
            "rawMessage": self.message,
            "span": {"module": self.span.module, "start": self.span.start, "end": self.span.end},
            "typescript": counterpart,
            "noTypeScriptCounterpart": if self.typescript.is_none() { self.no_typescript_counterpart } else { None },
        })
    }
}
