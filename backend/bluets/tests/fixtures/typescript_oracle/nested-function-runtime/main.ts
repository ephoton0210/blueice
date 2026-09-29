// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export function outer(base: number): number {
  function add(n: number): number { return n + base; }
  function twice(f: (n: number) => number, n: number): number { return f(f(n)); }
  function greet(name: string, punct: string = "!"): string { return "hi " + name + punct; }
  console.log(greet("a"));
  return twice(add, 1);
}
console.log(outer(10));
