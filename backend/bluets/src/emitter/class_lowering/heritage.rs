// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Move checked constructor aliases after module references have been rewritten.
use super::*;

pub(super) fn lower(module: &Module, edits: &mut Vec<TextEdit>) {
    for declaration in &module.declarations {
        let Declaration::Class(class) = declaration else {
            continue;
        };
        let Some(span) = &class.extends_span else {
            continue;
        };
        let Some(variable) = module.declarations.iter().find_map(|declaration| {
            let Declaration::Variable(variable) = declaration else {
                return None;
            };
            (variable.span.start == class.span.start
                && variable.span.end == variable.span.start
                && class.extends_name.as_ref() == Some(&variable.name))
            .then_some(variable)
        }) else {
            continue;
        };
        let mut local = Vec::new();
        edits.retain(|edit| {
            if edit.start >= span.start && edit.end <= span.end {
                local.push(edit.clone());
                false
            } else {
                true
            }
        });
        let source = super::render_span(&module.source, &local, span.start, span.end);
        edits.push(TextEdit {
            start: class.span.start,
            end: class.span.start,
            replacement: format!("const {} = {source};\n", variable.name),
        });
        edits.push(TextEdit {
            start: span.start,
            end: span.end,
            replacement: variable.name.clone(),
        });
    }
}
