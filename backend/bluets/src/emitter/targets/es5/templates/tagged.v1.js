// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Original cooked/raw site construction, matching the standard helper ABI.
function __blueice_target_template_object(cooked, raw) {
    if (Object.defineProperty) Object.defineProperty(cooked, "raw", { value: raw });
    else cooked.raw = raw;
    return cooked;
}
