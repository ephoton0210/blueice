// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const enum E { A = 1 << 2, B = A | 1, C = "x" + "y", D = E.B * 2, F = -E.A }
const n: number = E.D + E.F;
const s: string = E.C;
