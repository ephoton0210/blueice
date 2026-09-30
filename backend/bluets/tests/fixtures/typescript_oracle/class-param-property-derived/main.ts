// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    constructor(public id: number, protected tag: string) {}
}
class Derived extends Base {
    r: number = 9;
    constructor(public extra: number, private hidden: number) {
        super(1, "t");
    }
    total(): number { return this.id + this.extra + this.hidden + this.r; }
    name(): string { return this.tag; }
}
const d = new Derived(2, 3);
const n: number = d.total() + d.id + d.extra;
