// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace Shapes {
    export class Box {
        constructor(public w: number) {}
        area(): number { return this.w * this.w; }
    }
    export function unit(): Box { return new Box(1); }
}
const b: Shapes.Box = new Shapes.Box(3);
console.log(b.area(), Shapes.unit().area());
