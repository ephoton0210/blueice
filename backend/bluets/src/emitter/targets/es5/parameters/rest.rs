// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Native rest-array and loop phases keep distinct parameter boundary roles.
use crate::emitter::relocations::{source_anchor, unmapped};

pub(super) fn prologue(
    name: &str,
    index: &str,
    position: usize,
    start: usize,
    end: usize,
) -> String {
    let begin = source_anchor(start);
    let finish = source_anchor(end);
    let generated = unmapped();
    let destination = if position == 0 {
        index.to_string()
    } else {
        format!("{index} - {position}")
    };
    format!(
        "{begin}var {name} = [];{finish} {generated}for ({begin}var {index} = {position}{finish}; {begin}{index} < arguments.length{finish}; {begin}{index}++{finish}) {{ {begin}{name}[{destination}] = arguments[{index}];{finish} {generated}}}\n"
    )
}
