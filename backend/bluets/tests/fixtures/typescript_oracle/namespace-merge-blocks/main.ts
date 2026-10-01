// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export const a: number = 1;
    const hidden: number = 5;
}
namespace N {
    export const b: number = a + 1;
    export function sum(): number { return a + b; }
}
console.log(N.a, N.b, N.sum());
