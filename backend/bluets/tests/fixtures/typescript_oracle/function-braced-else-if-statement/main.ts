// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function label(value: number): string {
    if (value > 1) {
        return "many";
    } else if (value > 0) {
        return "one";
    } else {
        return "none";
    }
}

console.log(`${label(2)}:${label(1)}:${label(0)}`);
