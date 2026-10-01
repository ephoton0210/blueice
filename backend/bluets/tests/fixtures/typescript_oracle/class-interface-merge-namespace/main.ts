// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export interface Box { extra: string }
    export class Box { v: number = 3; }
}
const b: N.Box = new N.Box();
const s: string = b.extra;
console.log(b.v, s === undefined);
