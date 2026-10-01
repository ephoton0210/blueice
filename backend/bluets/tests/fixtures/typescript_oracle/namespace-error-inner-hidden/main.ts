// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export namespace Inner {
        const secret: number = 1;
        export const open: number = 2;
    }
}
const v: number = N.Inner.secret;
console.log(v);
