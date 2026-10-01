// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export class Base { x: number = 1; }
}
namespace M {
    export class D extends N.Base { y: number = 2; }
}
const d: M.D = new M.D();
const n: number = d.x + d.y;
console.log(n);
