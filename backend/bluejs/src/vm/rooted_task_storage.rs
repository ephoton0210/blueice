// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Rooted scheduling entries prepare complete storage before publication.

#[cfg(any(test, coverage))]
use super::*;

#[cfg(any(test, coverage))]
#[path = "../../tests/fixtures/rooted_task_storage.rs"]
mod ownership_tests;
