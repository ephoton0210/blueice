// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::normalized_exponential;

#[test]
fn exponential_formatting_normalizes_sign_and_optional_precision() {
    assert_eq!(normalized_exponential(123.5, None), "1.235e+2");
    assert_eq!(normalized_exponential(1.25, Some(2)), "1.25e+0");
    assert_eq!(normalized_exponential(0.00125, None), "1.25e-3");
    assert_eq!(normalized_exponential(0.0, None), "0e+0");
}
