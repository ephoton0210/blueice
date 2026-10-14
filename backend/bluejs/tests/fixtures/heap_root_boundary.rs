// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Root identifier exhaustion with all existing objects and roots left valid.

use super::*;

impl Heap {
    /// Move the monotonic serial forward so exactly this many registrations
    /// remain. Existing root identities are preserved, and no serial is reused.
    pub(crate) fn allow_root_registrations(&mut self, remaining: u64) {
        let next = u64::MAX - remaining;
        assert!(
            next >= self.next_root,
            "a root budget cannot reuse identifiers"
        );
        self.next_root = next;
    }
}
