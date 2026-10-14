// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
let calls = 0;
const box = {
  base: 20,
  add(this: {base: number}, value: number): number { return this.base + value; }
};
function receiver() { calls++; return box; }
const values: [number] = [22];
export const result = receiver().add(...values);
console.log(result, calls);
