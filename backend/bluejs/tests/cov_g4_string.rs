// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! String indexing beyond the largest `usize`.

mod cov_g4_common;
use cov_g4_common::failures;

#[test]
fn a_string_index_past_the_largest_usize_reads_as_missing() {
    assert_eq!(
        failures(
            r#"
eq('largest usize', 'abc'['18446744073709551615'], undefined);
eq('one past', 'abc'['18446744073709551616'], undefined);
eq('far past', 'abc'['99999999999999999999999'], undefined);
eq('in range', 'abc'['1'], 'b');
eq('leading zero', 'abc'['01'], undefined);
"#
        ),
        ""
    );
}
