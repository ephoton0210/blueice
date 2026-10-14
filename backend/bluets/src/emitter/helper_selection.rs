// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Helper policies admit only ABIs whose original lowering implements them.

use super::{CompilerOptions, Diagnostic, DiagnosticCode, EmittedJavaScript, Module, SourceSpan};

pub(super) fn validate(
    emitted: &EmittedJavaScript,
    module: &Module,
    options: &CompilerOptions,
) -> Result<(), Diagnostic> {
    if !options.import_helpers && !options.no_emit_helpers {
        return Ok(());
    }
    let original = crate::syntax::lex_with_limits(
        &module.id,
        &module.source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    let source_functions = original
        .windows(2)
        .filter(|pair| pair[0].is("function"))
        .map(|pair| pair[1].text.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &emitted.javascript,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut diagnostics| diagnostics.remove(0))?;
    for pair in tokens.windows(2) {
        let name = &pair[1].text;
        let target_helper = pair[0].is("function")
            && name.starts_with("__blueice_target_")
            && !source_functions.contains(name.as_str());
        let fixed_helper = matches!(pair[0].text.as_str(), "var" | "let" | "const")
            && matches!(
                name.as_str(),
                "__importDefault"
                    | "__importStar"
                    | "__bluetsClassPrivateGet"
                    | "__bluetsClassPrivateSet"
                    | "__bluetsClassPrivateIn"
                    | "__bluetsRunInitializers"
                    | "__bluetsEsDecorate"
                    | "__bluetsDecorate"
                    | "__bluetsParam"
                    | "__bluetsMetadata"
            );
        if target_helper || fixed_helper {
            return Err(Diagnostic::error(
                DiagnosticCode::UnsupportedSyntax,
                SourceSpan::new(&module.id, 0, 0),
                format!("helper selection does not support generated ABI `{name}`"),
            ));
        }
    }
    Ok(())
}
