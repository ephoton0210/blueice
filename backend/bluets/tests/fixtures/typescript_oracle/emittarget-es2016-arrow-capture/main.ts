// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
function calculate(base: number, ...values: number[]): number {
  const add = (value: number = 22): number => base + value;
  return add(values[0]);
}
export const result = calculate(20, 22);
console.log(result);
