// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Statement-compiler paths only reachable while a program runs: direct eval
//! code compiled against a caller whose variables an earlier eval created,
//! and name inference through parentheses.

mod cov_g8_common;

use cov_g8_common::observe;

const CASES: &[(&str, &str)] = &[
    (
        "(function () { eval('var ex = 1'); eval('var ex = 2'); return ex; })()",
        "2",
    ),
    (
        "(function () { eval('var ex = 1'); eval('var { ex = 5 } = {}; ex'); return ex; })()",
        "5",
    ),
    (
        "(function () { eval('var ex = 1'); eval('function ex() {}'); return typeof ex; })()",
        "function",
    ),
    (
        "(function () { try { throw 1; } catch (e) { eval('var e = 2'); return e; } })()",
        "2",
    ),
    (
        "(function () { var f = (function () {}); var g = ((() => 1)); var h = (class {}); return f.name + g.name + h.name; })()",
        "fgh",
    ),
    (
        "(function () { var o = {}; with (o) { var wx = 1; var { a: wy = 2 } = { a: undefined }; } return wx + wy; })()",
        "3",
    ),
];

#[test]
fn eval_code_and_parenthesized_names_behave() {
    for (expr, expected) in CASES {
        assert_eq!(observe(expr, false), *expected, "{expr}");
    }
}
