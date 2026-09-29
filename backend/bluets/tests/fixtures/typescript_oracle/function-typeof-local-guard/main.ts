// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function choose(input: string | number): number {
    const value: string | number = input;
    if (typeof value === 'string') { return 1; }
    return value;
}

function invert(input: string | number): number {
    const value: string | number = input;
    if (typeof value !== "string") { return value; }
    return 1;
}

console.log(`${choose('Ada')}:${choose(42)}:${invert('Ada')}:${invert(42)}`);
