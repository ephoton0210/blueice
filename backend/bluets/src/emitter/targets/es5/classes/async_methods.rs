// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An erased async modifier still owns its ES5 method's function signature.
use super::*;

#[derive(Default)]
pub(super) struct Signatures(Vec<(usize, usize)>);

impl Signatures {
    pub(super) fn retain(
        &mut self,
        emitted: &EmittedJavaScript,
        module: &Module,
        property: usize,
        signature: &str,
        member_start: usize,
        members: &str,
    ) {
        let Some(method) = async_functions::class_method_at(emitted, module, property) else {
            return;
        };
        // The descriptor is assembled here, before any original method body.
        // Record its exact generated signature prefix, not a rendered-name scan.
        let needle = format!("{signature}function (");
        if let Some(offset) = members[member_start..].find(&needle) {
            let keyword = member_start + offset + signature.len();
            self.0.push((keyword, method.span.start));
            if method.parameters.is_empty() {
                // An empty original signature retains the async entry phase
                // at its close and body entry, after each relocation marker.
                if let Some(close) = members[keyword..].find(") ") {
                    self.0.push((keyword + close, method.span.start));
                    if let Some(body) = members[keyword + close..].find('{') {
                        self.0.push((keyword + close + body, method.span.start));
                    }
                }
            }
        }
    }

    pub(super) fn positions(
        self,
        replacement: &str,
        members_start: usize,
        generated_start: usize,
        source: &str,
        edits: &[TextEdit],
    ) -> Vec<(usize, usize)> {
        self.0
            .into_iter()
            .map(|(offset, original)| {
                let prefix = &replacement[..members_start + offset];
                let generated = relocations::expand(prefix, source, edits)
                    .iter()
                    .map(|part| part.text().len())
                    .sum::<usize>();
                (generated_start + generated, original)
            })
            .collect()
    }
}
