// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace Outer {
    export let level: number = 1;
    export namespace Inner {
        export function bump(): number { level += 10; return level; }
    }
}
namespace Outer {
    export function read(): number { return level; }
}
console.log(Outer.Inner.bump(), Outer.read(), Outer.level);
