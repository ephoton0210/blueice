// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native tsc-style emission is separate from the fail-closed compile result.
use super::*;

impl Compilation {
    /// Emit supported syntax despite mapped semantic errors for a native CLI.
    /// This never changes `output`, acquires I/O authority, accepts owner/subset
    /// refusals, or permits strict-runtime emission. Options and source bytes
    /// must still match the checked compilation's fingerprint.
    pub fn emit_for_native_cli(
        &self,
        options: &CompilerOptions,
    ) -> Result<Option<emitter::BuildOutput>, Diagnostic> {
        if !super::isolated_declarations::validate_options(options).is_empty()
            || options.runtime_policy != RuntimePolicy::Checked
            || fingerprint(&self.project, options) != self.project_fingerprint
            || self.diagnostics.iter().any(|diagnostic| {
                diagnostic.typescript.is_none()
                    || !(matches!(
                        diagnostic.code,
                        DiagnosticCode::TypeMismatch
                            | DiagnosticCode::ReturnTypeMismatch
                            | DiagnosticCode::UnknownName
                            | DiagnosticCode::UnknownType
                            | DiagnosticCode::UsedBeforeDeclaration
                            | DiagnosticCode::DuplicateDeclaration
                            | DiagnosticCode::ImmutableAssignment
                    ) || jsx_runtime_type_diagnostic(diagnostic, &self.project))
            })
        {
            return Ok(None);
        }
        let Some(checked) = &self.checked else {
            return Ok(None);
        };
        emitter::emit(checked, &self.project, options).map(Some)
    }
}

fn jsx_runtime_type_diagnostic(diagnostic: &Diagnostic, project: &Project) -> bool {
    diagnostic.code == DiagnosticCode::ModuleNotFound
        && diagnostic
            .typescript
            .as_ref()
            .is_some_and(|counterpart| matches!(counterpart.code, 2875 | 7016))
        && project
            .modules
            .get(&diagnostic.span.module)
            .and_then(|module| crate::lex(&diagnostic.span.module, &module.source).ok())
            .is_some_and(|tokens| {
                tokens.iter().any(|token| {
                    token.kind == crate::TokenKind::JsxElement
                        && token.start == diagnostic.span.start
                        && token.end == diagnostic.span.end
                })
            })
}
