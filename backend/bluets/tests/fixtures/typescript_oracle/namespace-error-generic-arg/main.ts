// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export interface Box<T> { v: T }
}
const b: N.Box<number> = { v: "s" };
console.log(b);
