// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export function run(value: number): number {
  function twice(n: number): number { return n * 2; }
  const f = (n: number): number => twice(n);
  if (value > 0) { const value: number = 3; console.log(f(value)); }
  try { throw value; } catch (error) { console.log(typeof error); }
  for (const item of [1, 2]) { console.log(item); }
  return f(value);
}
export const seed: number = 3;
export type Seed = typeof seed;
export function echo(value: number): typeof value { return value; }
export const sample: Seed = echo(seed);
console.log(run(4), sample);
