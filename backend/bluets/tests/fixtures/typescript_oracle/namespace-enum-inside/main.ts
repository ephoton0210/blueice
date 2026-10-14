// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export enum Color { Red, Green }
    export const favorite: Color = Color.Green;
}
const c: N.Color = N.Color.Red;
console.log(c, N.favorite, N.Color.Green);
