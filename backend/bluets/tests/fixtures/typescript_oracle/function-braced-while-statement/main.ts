// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function sum(value: number): number {
    let total: number = 0;
    while (value > 0) {
        total += value;
        value -= 1;
    }
    return total;
}

console.log(`${sum(0)}:${sum(3)}`);
