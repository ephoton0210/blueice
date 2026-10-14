// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

enum M { A = 1, B = "b", C = 3 }
const x: M = M.B;
const n: number = M.A;
const s: string = M.B;
const back: string = M[1];
