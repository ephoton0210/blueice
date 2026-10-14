// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    const secret: number = 7;
    function helper(): number { return secret + 1; }
    export function reveal(): number { return helper(); }
}
console.log(N.reveal());
