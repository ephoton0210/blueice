// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const x: string = "top";
namespace N {
    const x: number = 5;
    export const y: number = x + 1;
}
console.log(x, N.y);
