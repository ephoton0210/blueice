// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace O {
    export const early: string = typeof O.late + "|" + typeof late;
    export function late(): number { return 1; }
    export const after: string = typeof O.late;
}
console.log(O.early, O.after);
