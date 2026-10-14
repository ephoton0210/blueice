// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace Outer {
    export function f(): number { return 1; }
    export namespace Inner {
        export function g(): number { return f() + 1; }
    }
}
console.log(Outer.Inner.g());
