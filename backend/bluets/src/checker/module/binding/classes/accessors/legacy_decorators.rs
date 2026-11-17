// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Accessor decorator errors are semantic even when no output is requested.

use super::*;

pub(super) fn validate(class: &ClassDeclaration, source: &str) -> Vec<Diagnostic> {
    let mut placed = BTreeMap::new();
    let mut diagnostics = Vec::new();
    for member in &class.members {
        let (Some(accessor), Some(decorator)) =
            (member.accessor.as_ref(), member.decorators.first())
        else {
            continue;
        };
        // Native rejects decorators on private identifiers and accessor
        // signatures before considering decoration of an accessor pair.
        if !accessor.body_present
            || member
                .key
                .first()
                .is_some_and(|key| key.text.starts_with('#'))
        {
            continue;
        }
        if placed
            .insert((accessor.is_static, &accessor.name), accessor.getter)
            .is_some_and(|getter| getter != accessor.getter)
        {
            // Native points to the second accessor's @ token. Attach its
            // exact position here so general decorator refinement cannot
            // select another decorator from a stacked list.
            let span = SourceSpan::new(
                &decorator.span.module,
                decorator.span.start,
                decorator.span.start + 1,
            );
            diagnostics.push(
                Diagnostic::error(
                    DiagnosticCode::UnsupportedSyntax,
                    span,
                    "decorators cannot be applied to both the getter and the setter of the same name",
                )
                .with_typescript(1207, Vec::new())
                .with_source_position(source),
            );
        }
    }
    diagnostics
}
