// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
type Pair = [number, string];
interface Holder { p: Pair; q: { r: [number, boolean]; s: number } }
const h: Holder = { p: [1, "a"], q: { r: [2, true], s: 3 } };
function takeHolder(x: Holder): number { return x.p[0] + x.q.r[0]; }
export const a: number = takeHolder({ p: [1, "a"], q: { r: [2, false], s: 4 } });
const rec: { p: [number, string] } = { p: [5, "e"] };
const list: [number, string][] = [[1, "a"], [2, "b"]];
const opt: { p?: [number, string] } = { p: [1, "x"] };
function make(): { p: [number, string] } { return { p: [7, "m"] }; }
export const b: number = rec.p[0] + list[1][0] + make().p[0] + (opt.p ? 1 : 0) + h.p[0];
