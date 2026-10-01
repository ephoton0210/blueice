// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export const enum K { A = 1, B = 2 }
}
const k: N.K = N.K.A;
console.log(k);
