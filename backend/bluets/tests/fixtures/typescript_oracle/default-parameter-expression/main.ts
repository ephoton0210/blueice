// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function add(left: number, right: number): number {
    return left + right;
}

function scale(value: number = add(20, 1), multiplier: number = 2): number {
    return value * multiplier;
}

console.log(scale(undefined));
