// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const shifted: number = 20 << 1;
const combined: number = ((shifted | 1) & 62) ^ 10;
const signed: number = 20 >> 1;
const unsigned: number = 20 >>> 1;
console.log(`${combined}:${signed}:${unsigned}`);
