// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Static assertion erasure retains original token positions for direct lowering.

use super::*;

impl Module {
    /// Original ranges containing an `as`, `satisfies` or const assertion's
    /// erased type syntax. Runtime consumers keep the operand's original tokens.
    pub fn erased_assertion_spans(&self) -> impl Iterator<Item = SourceSpan> + '_ {
        self.edits
            .iter()
            .filter(|edit| {
                edit.replacement.is_empty() && self.type_assertions.contains_key(&edit.start)
            })
            .map(|edit| SourceSpan::new(&self.id, edit.start, edit.end))
    }
}
