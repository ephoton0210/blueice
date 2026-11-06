// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Enum value text is synthesized; member boundaries retain their source owner.
use super::*;
use crate::parser::EnumDeclaration;

impl Mapper<'_> {
    pub(super) fn enumeration(&mut self, generated: &EnumDeclaration, source: &EnumDeclaration) {
        self.range(&generated.span, &source.span);
        self.range(&generated.name_span, &source.name_span);
        if self.exhausted {
            return;
        }
        let members: BTreeMap<_, _> = source
            .members
            .iter()
            .map(|member| (&member.name, member))
            .collect();
        for member in &generated.members {
            if self.exhausted {
                break;
            }
            let Some(original) = members.get(&member.name) else {
                continue;
            };
            self.range(&member.name_span, &original.name_span);
            self.point(member.span.end, original.span.end);
        }
    }
}
