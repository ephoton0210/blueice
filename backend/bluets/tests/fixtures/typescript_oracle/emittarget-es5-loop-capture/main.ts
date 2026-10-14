// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
export const results: number[] = [];
const functions: (() => number)[] = [];
for (let value = 20; value < 23; value++) { functions.push(() => value); }
for (const get of functions) { results.push(get()); }
console.log(results.join(","));
