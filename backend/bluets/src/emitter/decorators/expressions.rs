// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Generated decoration finishes before its enclosing original token resumes.
use super::*;

pub(in crate::emitter) fn unmapped_continuation(
    module: &Module,
    class: &ClassDeclaration,
    options: &CompilerOptions,
) -> bool {
    let Ok(tokens) = crate::lex(&module.id, &module.source) else {
        return false;
    };
    if tokens
        .iter()
        .find(|token| token.start >= class.span.end)
        .is_some_and(|token| token.is(")"))
    {
        return true;
    }
    if options.module_kind != crate::ModuleKind::CommonJs {
        return false;
    }
    crate::emitter::runtime_declarations(&module.declarations)
        .into_iter()
        .any(|(declaration, _)| {
            let Declaration::Variable(variable) = declaration else {
                return false;
            };
            variable.exported
                && !variable.declared
                && variable
                    .initializer
                    .iter()
                    .find(|token| !token.is("("))
                    .and_then(|token| module.class_expression(token.start))
                    .is_some_and(|expression| expression.class.span == class.span)
                && matches!(
                    crate::emitter::commonjs::class_values::storage(
                        module, variable, options, &tokens
                    ),
                    crate::emitter::commonjs::class_values::Storage::Direct
                )
        })
}
