// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export const a: number = 1;
    export function f(x: number): number { return x + a; }
    export interface I { q: number }
    export type T = string;
}
const x: number = N.a + N.f(2);
const i: N.I = { q: 1 };
const t: N.T = "v";
console.log(x, i.q, t);
