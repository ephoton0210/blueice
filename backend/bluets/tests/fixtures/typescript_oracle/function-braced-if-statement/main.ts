// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function label(value: number): string {
    if (value > 0) {
        return "positive";
    } else {
        return "other";
    }
}

console.log(`${label(2)}:${label(0)}`);
