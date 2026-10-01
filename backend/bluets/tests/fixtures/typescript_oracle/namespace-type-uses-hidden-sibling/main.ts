// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    interface H { q: number }
    export interface P { h: H }
    export function mk(): P { return { h: { q: 1 } }; }
}
const p: N.P = N.mk();
const q: number = p.h.q;
console.log(q);
