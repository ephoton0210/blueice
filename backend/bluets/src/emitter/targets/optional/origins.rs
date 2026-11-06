// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Logical property and fallback boundaries survive both optional-value ABIs.
use crate::emitter::relocations::source_anchor;

pub(super) fn property(
    receiver: &str,
    key: &str,
    temporary: &str,
    helper: &str,
    external: bool,
    (start, receiver_end, key_start, key_end): (usize, usize, usize, usize),
) -> String {
    let entry = source_anchor(start);
    let tested = source_anchor(receiver_end);
    let name = source_anchor(key_start);
    let end = source_anchor(key_end);
    if external {
        format!("{entry}({temporary}=({receiver}{tested}), {entry}{temporary}{tested}===null||{entry}{temporary}{tested}===void 0 ? void 0 : {entry}{temporary}{tested}[{name}{key}{end}]){end}")
    } else {
        format!("{entry}{helper}({receiver}{tested}, {name}{key}{end}){end}")
    }
}

pub(super) fn coalesce(
    left: &str,
    right: &str,
    temporary: &str,
    helper: &str,
    external: bool,
    (start, left_end, right_end): (usize, usize, usize),
) -> String {
    let entry = source_anchor(start);
    let tested = source_anchor(left_end);
    let end = source_anchor(right_end);
    let predicate = if external {
        format!("{temporary}===null||{temporary}===void 0")
    } else {
        format!("{helper}({temporary})")
    };
    format!("{entry}({temporary} = {left}{tested}, {predicate} ? ({right}{end}){tested} : {temporary}{end}){end}")
}
