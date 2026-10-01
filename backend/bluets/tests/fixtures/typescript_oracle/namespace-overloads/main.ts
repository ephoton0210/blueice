// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export function pick(a: number): number;
    export function pick(a: string): string;
    export function pick(a: number | string): number | string { return a; }
}
const n: number = N.pick(1);
const s: string = N.pick("a");
console.log(n, s);
