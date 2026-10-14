// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original helper: evaluate a receiver once and read only a non-nullish value.
function __blueice_target_optional_property(value, key) {
    return value === null || value === void 0 ? void 0 : value[key];
}
