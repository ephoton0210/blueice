// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Run the private-name AST cases through the public compiler boundary.

use blueice_bluejs::*;

#[path = "../src/compiler/private_validation/tests.rs"]
mod private_validation_cases;
