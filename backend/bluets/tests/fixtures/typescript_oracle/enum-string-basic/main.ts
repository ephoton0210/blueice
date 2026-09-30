// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

enum S { A = "a", B = "b" }
const s: string = S.A;
const e: S = S.B;
const label: string = S["A"];
