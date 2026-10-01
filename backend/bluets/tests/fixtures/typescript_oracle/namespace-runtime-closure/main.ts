// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace C {
    export let x: number = 1;
    export const getter = (): number => x;
    export function assign(v: number): void { x = v; }
}
const g = C.getter;
C.assign(9);
console.log(g(), C.x);
