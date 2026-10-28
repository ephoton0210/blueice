// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Declaration naming failures identify the exported declaration and owned origin.
use super::*;

impl Context<'_> {
    pub(super) fn cannot_name(&self, name: &str, owner: &str, span: &SourceSpan) -> Diagnostic {
        let declaration = self
            .module
            .declarations
            .iter()
            .find(|declaration| declaration.span().start == span.start);
        let (code, exported) = match declaration {
            Some(Declaration::Function(function)) => (4058, function.name.as_str()),
            Some(Declaration::Variable(variable)) => (4023, variable.name.as_str()),
            _ => (4023, "<export>"),
        };
        let origin = strip_extension(owner);
        let mut arguments = if code == 4023 {
            vec![exported.into(), name.into()]
        } else {
            vec![name.into()]
        };
        arguments.push(serde_json::to_string(origin).expect("module spelling"));
        let tokens = crate::lex(&self.module.id, &self.module.source).unwrap_or_default();
        let location = tokens
            .iter()
            .find(|token| {
                span.start <= token.start && token.end <= span.end && token.text == exported
            })
            .map_or_else(|| span.clone(), |token| token.span(&self.module.id));
        Diagnostic::error(
            DiagnosticCode::TypeMismatch,
            location,
            format!(
                "TS{code}: exported declaration uses external type `{name}` which cannot be named"
            ),
        )
        .with_typescript(code, arguments)
    }
}

fn strip_extension(name: &str) -> &str {
    for suffix in [
        ".d.ts", ".d.mts", ".d.cts", ".tsx", ".ts", ".mts", ".cts", ".jsx", ".js", ".mjs", ".cjs",
    ] {
        if let Some(name) = name.strip_suffix(suffix) {
            return name;
        }
    }
    name
}
