// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
async function load(n: number): Promise<number> { return n * 2; }
export const a: number = await load(1);
const b: number = await load(2);
const c = await load(3);
export const d: number = a + b + c;
const results: number[] = [];
for (const n of [1, 2]) { results.push(await load(n)); }
if (b > 0) { const e: number = await load(4); results.push(e); }
export const f: number = results.length;
