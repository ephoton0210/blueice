// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! RegExp's character-class-escape fast path (every escape, anchored and
//! repeated forms, code points and lone surrogates), escapes in group names
//! and the legacy `RegExp.$1`-style statics.

mod cov_g8_common;

use cov_g8_common::check_cases;

include!("cov_g8_tables/regexp.in");

#[test]
fn regular_expressions_report_the_reference_results() {
    check_cases(REGEXP_CASES, false);
}
