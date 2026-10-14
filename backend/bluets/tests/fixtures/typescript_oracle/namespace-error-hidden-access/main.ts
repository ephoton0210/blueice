// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    const hidden: number = 1;
    export const shown: number = 2;
}
const h: number = N.hidden;
const s: number = N.shown;
console.log(h, s);
