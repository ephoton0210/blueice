// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Call transport borrows logical operand boundaries; helper bodies do not.
use super::{EmittedJavaScript, TextEdit};
use crate::emitter::relocations;

pub(super) fn invoke(
    helper: &str,
    callee: &str,
    arguments: &str,
    external: bool,
    start: usize,
    callee_end: usize,
) -> String {
    let entry = relocations::source_anchor(start);
    let tested = relocations::source_anchor(callee_end);
    let prefix = if external {
        format!("{}{helper}{entry}", relocations::unmapped())
    } else {
        format!("{entry}{helper}")
    };
    format!("{prefix}(({callee}{tested}), void 0, {arguments})")
}

/// The lowered bound construction completes at its argument list's last operand,
/// before the original closing parenthesis. Later member syntax keeps that edge.
pub(super) fn complete_construction(
    previous: EmittedJavaScript,
    edits: Vec<TextEdit>,
    boundary: usize,
    operand_end: usize,
) -> EmittedJavaScript {
    super::super::super::mapped_fragments::with_completion(previous, edits, boundary, operand_end)
}
