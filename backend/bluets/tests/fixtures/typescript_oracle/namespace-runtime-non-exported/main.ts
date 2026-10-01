// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace H {
    let hidden: number = 0;
    export function next(): number { hidden = hidden + 1; return hidden; }
}
console.log(H.next(), H.next(), Object.keys(H));
