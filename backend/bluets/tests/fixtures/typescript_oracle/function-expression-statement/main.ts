// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

let total: number = 0;

function remember(value: number): number {
    total += value;
    return total;
}

remember(2);
console.log(`${remember(3)}:${total}`);
