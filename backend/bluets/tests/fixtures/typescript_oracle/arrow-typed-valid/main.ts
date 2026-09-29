// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export function outer(a: string, k: number): number {
  const inner = (n: number): number => n + k;
  const named = (s: string): string => s + a;
  return inner(1) + named("x").length;
}
export const g: (n: number) => number = (n: number): number => n;
export const h: (n: number) => number = (n) => n + 1;
export const items = [1, 2].map((n: number): number => n + 1);
