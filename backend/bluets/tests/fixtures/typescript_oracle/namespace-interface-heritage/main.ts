// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export interface Base { id: number }
    export interface Child extends Base { name: string }
}
const c: N.Child = { id: 1, name: "n" };
console.log(c.id, c.name);
