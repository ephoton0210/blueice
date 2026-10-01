// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace Outer {
    export const base: number = 10;
    export namespace Inner {
        export const z: number = base * 2;
        export function twice(n: number): number { return n * 2; }
    }
    export const w: number = Inner.twice(Inner.z);
}
namespace A.B.C {
    export const deep: string = "deep";
}
console.log(Outer.Inner.z, Outer.w, Outer.Inner.twice(4), A.B.C.deep);
