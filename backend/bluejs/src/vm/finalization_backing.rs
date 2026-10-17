// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Registry cell slots belong to their heap through retained capacity.

#[cfg(any(test, coverage))]
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/finalization_backing.rs"]
mod ownership_tests;
