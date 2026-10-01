// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace Outer {
    export interface Shape { sides: number }
    export namespace Inner {
        export function make(): Shape { return { sides: 3 }; }
    }
}
const s: Outer.Shape = Outer.Inner.make();
console.log(s.sides);
