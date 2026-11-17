// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Original evaluation and completion phases can own inserted indentation.

use super::*;

pub(super) fn source_owners(
    module: &Module,
    options: &CompilerOptions,
) -> Result<BTreeSet<(usize, usize)>, Diagnostic> {
    let mut offsets = BTreeSet::new();
    if options.module_kind == crate::ModuleKind::CommonJs {
        // An inline star forwarder is one original publication operation.
        // Its existing owner continues through formatter-added line breaks.
        // System transport keeps explicit unmapped states, which this policy
        // cannot turn into source grants.
        offsets.extend(
            module
                .declarations
                .iter()
                .filter_map(|declaration| match declaration {
                    Declaration::ValueExport(export)
                        if export.star && export.namespace.is_none() && !export.is_type_only() =>
                    {
                        Some(export.span.start)
                    }
                    _ => None,
                }),
        );
    }
    if options.target >= crate::EcmaTarget::Es2015 && options.target < crate::EcmaTarget::Es2018 {
        // A lowered async generator's driver and outer function close share
        // the original function endpoint, including any wrapper indentation.
        offsets.extend(
            super::super::runtime_declarations(&module.declarations)
                .into_iter()
                .filter_map(|(declaration, _)| match declaration {
                    Declaration::Function(function)
                        if function.async_function && function.generator =>
                    {
                        Some(function.span.end)
                    }
                    _ => None,
                }),
        );
        offsets.extend(
            module
                .nested_functions
                .values()
                .filter(|function| function.async_function && function.generator)
                .map(|function| function.span.end),
        );
    }
    if module.class_expressions.is_empty() && offsets.is_empty() {
        return Ok(BTreeSet::new());
    }
    let tokens = crate::syntax::lex_with_limits(
        &module.id,
        &module.source,
        options.limits.parser.max_source_bytes,
        options.limits.parser.max_tokens,
    )
    .map_err(|mut errors| errors.remove(0))?;
    offsets.extend(module.class_expressions.values().filter_map(|expression| {
        if expression.statement {
            Some(
                super::super::class_lowering::expressions::names::declaration_name_end_origin(
                    &expression.class,
                    &tokens,
                    options.target == crate::EcmaTarget::Es5,
                ),
            )
        } else {
            super::super::class_lowering::expressions::names::evaluation_prefix_origin(
                expression, &tokens,
            )
        }
    }));
    let mut pending = offsets.into_iter().peekable();
    let mut owners = BTreeSet::new();
    let (mut line, mut column) = (0, 0);
    let mut chars = module.source.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if pending.peek() == Some(&offset) {
            owners.insert((line, column));
            pending.next();
        }
        if pending.peek().is_none() {
            break;
        }
        if is_line_break(ch, chars.peek().map(|(_, next)| *next)) {
            line += 1;
            column = 0;
        } else {
            column += ch.len_utf16();
        }
    }
    Ok(owners)
}
