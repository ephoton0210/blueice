// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The String algorithms in `native.rs` that build their result piece by
//! piece: each piece is charged against the string byte limit, so every limit
//! from zero up must either finish or report `StringLimit`.

mod cov_g7_common;
use cov_g7_common::sweep_strings;

#[test]
fn building_strings_piece_by_piece_reports_the_string_limit() {
    for body in [
        "String.fromCharCode(65, 66, 67); String.fromCodePoint(0x1F600, 66);",
        "'abcdefgh'.concat('ijklmnop', 'qrstuvwx');",
        "'x'.anchor('y\"z'); 'x'.big(); 'x'.link('u');",
    ] {
        sweep_strings(body, 70);
    }
}
