// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace V {
    export let x: number;
    export function assign(v: number): void { x = v; }
    export function read(): number { return x; }
}
console.log(V.x, V.read());
V.assign(4);
console.log(V.x, V.read());
