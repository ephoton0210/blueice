// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
function sum(...values: number[]): number { return values.reduce((left, right) => left + right, 0); }
const values = [20, 22];
export const result = sum(...values);
console.log(result);
