// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace A.B {
    export const x: number = 1;
}
namespace A.B {
    export const y: number = x + 1;
}
const z: number = A.B.y;
console.log(z);
