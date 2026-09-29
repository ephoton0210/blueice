// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export function outer(a: string, k: number): number {
  const inner = function (n: number): number { return n + k; };
  const named = function label(s: string): string { return s + a; };
  return inner(1) + named("x").length;
}
export const g: (n: number) => number = function (n: number): number { return n; };
export const h: (n: number) => number = function (n) { return n + 1; };
