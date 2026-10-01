// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function g(x: number): number { return x + 1; }
namespace g {
    export const meta: string = "m";
    export function twice(x: number): number { return g(g(x)); }
}
console.log(g(1), g.meta, g.twice(1));
