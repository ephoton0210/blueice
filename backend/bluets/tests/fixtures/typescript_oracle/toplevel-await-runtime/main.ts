// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
async function load(n: number): Promise<number> { return n * 2; }
async function later(n: number): Promise<number> { await load(0); return n + 1; }
export const a: number = await load(1);
const b: number = await later(2);
const results: number[] = [];
for (const n of [1, 2, 3]) { results.push(await load(n)); }
if (b > 0) { results.push(await later(10)); }
console.log("start");
console.log(a, b, results.length, results[3]);
