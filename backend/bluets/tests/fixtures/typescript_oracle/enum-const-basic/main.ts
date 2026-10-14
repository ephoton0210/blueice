// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const enum E { A = 1, B, S = "s", "x-y" = 7 }
const a: number = E.A;
const b: number = E["B"] + E["x-y"];
const s: string = E.S;
function f(): number { return E.B * 2; }
const t: E.A = E.A;
const u: E = E.B;
const n: number = E.A + E.B + f();
