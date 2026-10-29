// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Template substitutions retain lexical scope and raw literal spelling.
use super::*;

pub(super) fn template(
    source: &str,
    token: &Token,
    module: &Module,
    depth: usize,
    mut transform: impl FnMut(&str, usize) -> Result<String, Diagnostic>,
) -> Result<String, Diagnostic> {
    if depth == 0 {
        return Err(Diagnostic::error(
            DiagnosticCode::ResourceLimit,
            token.span(&module.id),
            "template suspension exceeds the depth limit",
        ));
    }
    let bytes = source.as_bytes();
    let mut at = token.start + 1;
    let mut edits = Vec::new();
    while at + 1 < token.end {
        if bytes[at] == b'\\' {
            at += 2;
            continue;
        }
        if bytes[at..].starts_with(b"${") {
            let start = at + 2;
            let (end, _) =
                crate::syntax::lex_expression(&module.id, source, start).map_err(|message| {
                    Diagnostic::error(DiagnosticCode::ParseError, token.span(&module.id), message)
                })?;
            edits.push(TextEdit {
                start: start - token.start,
                end: end - token.start,
                replacement: transform(&source[start..end], depth - 1)?,
            });
            at = end + 1;
        } else {
            at += 1;
        }
    }
    Ok(apply_edits(&source[token.start..token.end], edits).javascript)
}
