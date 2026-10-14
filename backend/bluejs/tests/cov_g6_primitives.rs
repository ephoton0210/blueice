// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Numeric conversion and comparison of values that have no number.
mod cov_g6_support;

use cov_g6_support::check;

const SYMBOL_TO_NUMBER: &str = "throw:TypeError:Number conversion requires a non-Symbol primitive";

#[test]
fn a_symbol_has_no_numeric_value() {
    check(&[
        ("-Symbol()", SYMBOL_TO_NUMBER),
        ("Symbol() < 1", SYMBOL_TO_NUMBER),
        ("1 < Symbol()", SYMBOL_TO_NUMBER),
    ]);
}
