// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Decorator helpers belong to the module; exported classes belong to their namespace.

use crate::parser::{ClassDeclaration, Declaration, Module};

pub(in crate::emitter) fn namespace_owner<'a>(
    module: &'a Module,
    class: &ClassDeclaration,
) -> Option<(&'a str, usize)> {
    fn find<'a>(
        declarations: &'a [Declaration],
        class: &ClassDeclaration,
        outer_start: Option<usize>,
    ) -> Option<(&'a str, usize)> {
        for declaration in declarations {
            let Declaration::Namespace(namespace) = declaration else {
                continue;
            };
            if namespace.span.start <= class.span.start && class.span.end <= namespace.span.end {
                let start = outer_start.unwrap_or(namespace.span.start);
                return find(&namespace.body, class, Some(start))
                    .or(Some((&namespace.name, start)));
            }
        }
        None
    }
    find(&module.declarations, class, None)
}

/// Insert helpers before the outer statement, never inside a class expression.
pub(in crate::emitter) fn helper_start(module: &Module, class: &ClassDeclaration) -> usize {
    if let Some((_, start)) = namespace_owner(module, class) {
        return start;
    }
    for declaration in &module.declarations {
        let span = match declaration {
            Declaration::Variable(value) => &value.span,
            Declaration::Function(value) => &value.span,
            Declaration::Class(value) => &value.span,
            _ => continue,
        };
        if span.start <= class.span.start && class.span.end <= span.end {
            return span.start;
        }
    }
    class.span.start
}
