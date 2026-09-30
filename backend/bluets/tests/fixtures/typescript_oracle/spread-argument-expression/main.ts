// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function add(left: number, right: number): number {
    return left + right;
}

const pair: [number, number] = [40, 2];
const empty: [] = [];
const value = new Object(...empty);
console.log(`${add(...pair)}:${typeof value}`);
