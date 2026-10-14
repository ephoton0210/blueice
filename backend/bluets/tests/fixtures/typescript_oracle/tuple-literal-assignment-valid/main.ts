// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
let p: [number, string] = [1, "a"];
p = [2, "b"];
let h: { p: [number, string] } = { p: [1, "a"] };
h = { p: [3, "c"] };
h.p = [4, "d"];
let list: [number, string][] = [[1, "a"]];
list = [[2, "b"]];
list[0] = [3, "c"];
let u: [number, string] | undefined;
u = [1, "x"];
let n: number;
n = p[0] + 1;
export const a: number = p[0] + h.p[0] + list[0][0] + (u ? u[0] : 0) + n;
