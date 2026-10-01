// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const base: number = 100;
function outer(n: number): number { return n + base; }
namespace N {
    export const v: number = outer(1);
    export namespace M {
        export const w: number = v + 1;
    }
}
console.log(N.v, N.M.w);
