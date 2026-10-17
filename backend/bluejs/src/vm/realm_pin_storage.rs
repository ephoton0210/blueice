// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Foreign callable pins retain their native storage before publication.

#[cfg(any(test, coverage))]
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/realm_pin_storage.rs"]
mod ownership_tests;
