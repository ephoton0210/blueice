// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export interface Box<T> { v: T }
    export function wrap<T>(v: T): Box<T> { return { v }; }
}
const b: N.Box<number> = N.wrap(1);
const n: number = b.v;
console.log(n);
