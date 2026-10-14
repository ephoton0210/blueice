// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export function outer(a: string, k: number): number {
  function inner(n: number): number { return n + k; }
  function label(s: string): string { return s + a; }
  function fact(n: number): number { return n ? n * fact(n - 1) : 1; }
  return inner(1) + label("x").length + fact(3);
}
