// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Finalization readiness and scheduling retain admitted native storage.

#[cfg(any(test, coverage))]
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/finalization_storage.rs"]
mod ownership_tests;
