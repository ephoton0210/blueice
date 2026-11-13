// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Source parser entry points and shared work limits.

use super::*;

pub(crate) struct ParseFailure {
    pub diagnostics: Vec<Diagnostic>,
    pub recovered: Option<Box<RecoveredSyntax>>,
}

pub(crate) struct RecoveredSyntax {
    pub module: Module,
    pub primitive_values: Vec<Token>,
}

/// Fatal syntax remains excluded from the graph. Only recovery references are
/// retained for the compiler's separate, owner-supplied diagnostic context.
pub(crate) fn parse_module_for_compilation(
    id: String,
    source: String,
    limits: ParserLimits,
) -> Result<Module, ParseFailure> {
    match parse_module_with_limits(id.clone(), source.clone(), limits.clone()) {
        Ok(module) => Ok(module),
        Err(diagnostics) => {
            let recovered = if diagnostics.iter().any(|diagnostic| {
                diagnostic
                    .typescript
                    .as_ref()
                    .is_some_and(|value| value.code == 1389)
            }) {
                lex_with_limits(&id, &source, limits.max_source_bytes, limits.max_tokens)
                    .ok()
                    .and_then(|tokens| {
                        Parser::new(id, source, tokens, limits.max_type_depth)
                            .parse_module_recovered()
                            .err()
                    })
                    .and_then(|failure| failure.recovered)
            } else {
                None
            };
            Err(ParseFailure {
                diagnostics,
                recovered,
            })
        }
    }
}

/// Parses one source record.  The caller owns resolution and can decide which
/// source records are authorized to participate in the project.
pub fn parse_module(
    id: impl Into<String>,
    source: impl Into<String>,
) -> Result<Module, Vec<Diagnostic>> {
    parse_module_with_limits(id, source, ParserLimits::default())
}

pub(crate) fn parse_module_with_limits(
    id: impl Into<String>,
    source: impl Into<String>,
    limits: ParserLimits,
) -> Result<Module, Vec<Diagnostic>> {
    parse_module_with_namespaces(id, source, limits, &[])
}

/// Parses a module that imports namespaces, given each by its local name and
/// what it exports, so a reference to one of their members is one token.
pub(crate) fn parse_module_with_namespaces(
    id: impl Into<String>,
    source: impl Into<String>,
    limits: ParserLimits,
    imported_namespaces: &[(String, NamespaceTree)],
) -> Result<Module, Vec<Diagnostic>> {
    let id = id.into();
    let source = source.into();
    let tokens = if limits == ParserLimits::default() {
        lex(&id, &source)?
    } else {
        lex_with_limits(&id, &source, limits.max_source_bytes, limits.max_tokens)?
    };
    // A module with namespaces is parsed twice: the first pass learns which
    // names are namespaces and what they export, and the second merges each
    // qualified reference (`N.x`) into one token before parsing again.
    let first = Parser::new(
        id.clone(),
        source.clone(),
        tokens.clone(),
        limits.max_type_depth,
    )
    .parse_module()?;
    let token_count = tokens.len().saturating_sub(1);
    let mut module = if let Some(names) =
        namespace_names::NamespaceNames::collect(&first.declarations, imported_namespaces)
    {
        Parser::new(id, source, tokens, limits.max_type_depth)
            .with_namespace_names(&names)
            .parse_module()?
    } else {
        first
    };
    if is_javascript(&module.id) {
        jsdoc::apply(&mut module, &limits, token_count)?;
    }
    Ok(module)
}
