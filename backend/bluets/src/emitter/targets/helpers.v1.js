// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original helper: strict nullish testing performs no coercion or user call.
function __blueice_target_is_nullish(value) {
    return value === null || value === void 0;
}
