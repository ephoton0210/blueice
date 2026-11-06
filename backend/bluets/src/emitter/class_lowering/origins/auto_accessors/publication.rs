// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The initial module publication precedes legacy auto-storage initialization.

use super::*;
use crate::compiler::{CompilerOptions, EcmaTarget, ModuleKind};

pub(in crate::emitter::class_lowering) fn retain_publication_order(
    original: &Module,
    owned: &Module,
    previous: &EmittedJavaScript,
    options: &CompilerOptions,
    edits: &mut [TextEdit],
) {
    if !options.experimental_decorators
        || options.target < EcmaTarget::Es2015
        || options.target >= EcmaTarget::Es2022
        || options.module_kind != ModuleKind::CommonJs
    {
        return;
    }
    for source in classes(original).into_iter().filter(|class| {
        class.exported
            && !class.anonymous
            && !crate::emitter::legacy_decorators::lowers_class(class)
            && class.members.iter().any(|member| {
                member
                    .field
                    .as_ref()
                    .is_some_and(|field| field.accessor && !field.declared)
            })
    }) {
        let expected = position(&original.source, source.span.start);
        let candidates = owned
            .declarations
            .iter()
            .enumerate()
            .filter_map(|(index, declaration)| {
                let Declaration::Class(class) = declaration else {
                    return None;
                };
                let keyword = crate::emitter::decorators::class_keyword_start(class, &owned.source);
                (class.name == source.name && mapped(previous, keyword) == Some(expected))
                    .then_some((index, class))
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            continue;
        }
        let (index, class) = candidates[0];
        let Some(Declaration::Raw(publication)) = owned.declarations.get(index + 1) else {
            continue;
        };
        let tokens = &publication.tokens;
        if !owned.source[class.span.end..publication.span.start]
            .trim()
            .is_empty()
            || tokens.len() != 5
            || !tokens[0].is("exports")
            || !tokens[1].is(".")
            || !tokens[2].is(source.export_name())
            || !tokens[3].is("=")
            || !tokens[4].is(&class.name)
            || &owned.source[tokens[4].end..publication.span.end] != ";"
        {
            continue;
        }
        let expected = previous
            .segment_limit
            .map(|_| position(&original.source, source.span.start));
        if mapped(previous, publication.span.start) != expected {
            continue;
        }
        let pending = edits
            .iter_mut()
            .filter(|edit| edit.start == class.span.end && edit.end == edit.start)
            .collect::<Vec<_>>();
        // This is the single after_class insertion produced by the class
        // lowering pass; no source or decorator statement may be crossed.
        if pending.len() == 1 {
            let edit = pending.into_iter().next().unwrap();
            edit.start = publication.span.end;
            edit.end = publication.span.end;
        }
    }
}
