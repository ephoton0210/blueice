// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

#[test]
fn bounded_values_policy_is_independent_of_static_metadata() {
    let options = CoreLaunchOptions::default()
        .with_debugger_endpoint(PathBuf::from("/tmp/debugger.sock"))
        .with_debugger_bounded_values();
    assert!(options.debugger_bounded_values);
    assert!(!options.debugger_static_metadata_inventory);
}
