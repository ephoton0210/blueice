// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class K {
    static s: number = 1;
    v: number = 2;
}
namespace K {
    export const t: number = 3;
    export function make(): K { return new K(); }
}
console.log(K.s, K.t, K.make().v);
