// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Legacy decorators receive an ordinary accessor descriptor after storage lowering.

use super::*;
use crate::compiler::EcmaTarget;
use std::collections::BTreeSet;

pub(super) fn lower(
    module: &Module,
    options: &CompilerOptions,
    edits: &mut Vec<TextEdit>,
) -> Result<(), Diagnostic> {
    // ESNext retains auto-accessor proposal syntax independently of the legacy
    // decorator calls emitted after the class.
    if options.target == EcmaTarget::EsNext {
        return Ok(());
    }
    let tokens = crate::lex(&module.id, &module.source).unwrap_or_default();
    let mut used: BTreeSet<_> = tokens
        .iter()
        .map(|token| token.text.trim_start_matches('#').to_string())
        .collect();
    let mut sequence = 0;
    let declarations = super::super::runtime_declarations(&module.declarations);
    let classes = declarations
        .iter()
        .filter_map(|(declaration, _)| {
            if let Declaration::Class(class) = declaration {
                Some(class)
            } else {
                None
            }
        })
        .chain(
            module
                .class_expressions
                .values()
                .map(|expression| &expression.class),
        );
    for class in classes {
        for shell in &class.members {
            let Some(field) = shell.field.as_ref().filter(|field| field.accessor) else {
                continue;
            };
            if shell.key.first().is_some_and(|token| token.is("[")) {
                return Err(unsupported(
                    &field.span,
                    "legacy computed auto-accessor keys need a once-only key binding",
                ));
            }
            let storage = loop {
                sequence += 1;
                let candidate = format!("_bluetsAccessorStorage{sequence}");
                if used.insert(candidate.clone()) {
                    break format!("#{candidate}");
                }
            };
            let key =
                &module.source[shell.key.first().unwrap().start..shell.key.last().unwrap().end];
            let prefix = if field.is_static { "static " } else { "" };
            let initializer = field.initializer.as_ref().map_or_else(
                || "void 0".into(),
                |tokens| {
                    super::super::decorators::relocated(
                        tokens.first().unwrap().start,
                        tokens.last().unwrap().end,
                    )
                },
            );
            edits.push(TextEdit {
                start: shell.span.start,
                end: shell.span.end,
                replacement: format!("{prefix}{storage} = {initializer}; {prefix}get {key}() {{ return this.{storage}; }} {prefix}set {key}(value) {{ this.{storage} = value; }}"),
            });
        }
    }
    Ok(())
}
