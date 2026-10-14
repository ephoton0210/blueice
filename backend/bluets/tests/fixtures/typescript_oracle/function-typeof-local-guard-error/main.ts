// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function takesNumber(value: number): void {}
function takesString(value: string): void {}

function bad(input: string | number): void {
    const value: string | number = input;
    if (typeof value === 'string') {
        takesNumber(value);
    } else {
        takesString(value);
    }
}
