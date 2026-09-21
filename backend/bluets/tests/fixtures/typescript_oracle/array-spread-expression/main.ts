// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const suffix: number[] = [2, 3];
const values: number[] = [1, ...suffix, 4];
console.log(`${values.length}:${values[2]}`);
