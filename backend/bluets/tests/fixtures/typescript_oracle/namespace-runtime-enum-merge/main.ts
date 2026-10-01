// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace M {
    export enum E { A, B }
}
namespace M {
    export enum E { C = 5 }
    export const v: E = E.C;
}
console.log(M.E.A, M.E.B, M.E.C, M.v, M.E[5]);
