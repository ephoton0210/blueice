// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function run(value: number): number {
    let observed: number = 0;
    try {
        if (value > 0) { throw value; }
        observed = 1;
    } catch (caught) {
        if (caught === 2) { observed = 3; }
    } finally {
        observed += 4;
    }
    return observed;
}

console.log(`${run(2)}:${run(0)}`);
