// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class A {
  constructor(p: [boolean]) {}
  m(p: [number, string]): number { return p[0]; }
  static s(p: [number]): number { return p[0]; }
}
const o = new A([true]);
export const a: number = o.m([1, "a"]) + A.s([1]);
class B extends A { constructor() { super([false]); } }
