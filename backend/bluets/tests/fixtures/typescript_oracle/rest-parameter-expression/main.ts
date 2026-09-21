// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function sum(base: number, ...values: number[]): number {
    return base + values[0];
}

const pair: [number] = [2];
console.log(sum(40, ...pair));
