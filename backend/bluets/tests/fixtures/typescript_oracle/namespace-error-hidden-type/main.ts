// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    interface H { q: number }
    export interface P { q: number }
}
const h: N.H = { q: 1 };
console.log(h);
