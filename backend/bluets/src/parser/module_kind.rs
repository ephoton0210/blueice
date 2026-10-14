// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Erased type imports/exports still distinguish an external module from a script.

use super::{module_declarations::has_module_syntax, Module};

impl Module {
    pub(crate) fn is_external_module(&self) -> bool {
        has_module_syntax(&self.declarations)
    }
}
