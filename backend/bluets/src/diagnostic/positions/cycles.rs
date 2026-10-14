// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A circular inference diagnostic addresses the first binding in its cycle.
use crate::parser::Declaration;
use crate::{Diagnostic, Project, SourceSpan};
use std::collections::BTreeSet;

pub(super) fn refine(project: &Project, diagnostic: &mut Diagnostic) {
    if !diagnostic
        .message
        .contains("initializer depends on a circular import")
        || diagnostic
            .typescript
            .as_ref()
            .is_none_or(|d| d.code != 7022)
    {
        return;
    }
    let origin = &diagnostic.span.module;
    let forward = reachable(project, origin, false);
    let reverse = reachable(project, origin, true);
    for id in forward.intersection(&reverse) {
        let Some(module) = project.modules.get(id) else {
            continue;
        };
        for declaration in &module.declarations {
            let Declaration::Variable(variable) = declaration else {
                continue;
            };
            if variable.annotation.is_some() {
                continue;
            }
            let references_cycle = module.declarations.iter().any(|declaration| {
                let Declaration::Import(import) = declaration else {
                    return false;
                };
                project
                    .resolved_import(id, import)
                    .is_some_and(|target| forward.contains(target) && reverse.contains(target))
                    && import.bindings.iter().any(|binding| {
                        !binding.type_only
                            && variable
                                .initializer
                                .iter()
                                .any(|token| token.is(&binding.local))
                    })
            });
            if !references_cycle {
                continue;
            }
            let Ok(tokens) = crate::syntax::lex(id, &module.source) else {
                continue;
            };
            let Some(name) = tokens.iter().find(|token| {
                token.start >= variable.span.start
                    && token.end <= variable.span.end
                    && token.is(&variable.name)
            }) else {
                continue;
            };
            let span = SourceSpan::new(id, name.start, name.end);
            diagnostic.typescript = super::super::mapping::build(
                7022,
                &span,
                vec![variable.name.clone()],
                &diagnostic.message,
            )
            .map(Box::new);
            return;
        }
    }
}

fn reachable(project: &Project, origin: &str, reverse: bool) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut pending = vec![origin.to_string()];
    while let Some(id) = pending.pop() {
        if !result.insert(id.clone()) {
            continue;
        }
        for (from, to) in project.resolution_edges() {
            let (source, target) = if reverse { (to, from) } else { (from, to) };
            if source == id {
                pending.push(target.to_string());
            }
        }
    }
    result
}
