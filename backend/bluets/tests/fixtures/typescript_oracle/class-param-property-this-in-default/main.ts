// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box {
    constructor(public a: number, public b: number = 2) {}
    sum(): number { return this.a + this.b; }
}
const n: number = new Box(1).sum() + new Box(1, 5).sum();
