// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export interface A { a: number }
    export interface B { b: string }
    export type U = A | B;
    export function isA(u: U): boolean { return "a" in u; }
}
const u: N.U = { a: 1 };
console.log(N.isA(u));
