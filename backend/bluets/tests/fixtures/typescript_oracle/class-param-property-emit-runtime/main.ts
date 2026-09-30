// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class Base {
    z: number = 5;
    constructor(public x: number, private y: number, protected readonly w: number = 3, readonly v?: string) {
        this.z = this.z + this.x;
    }
    sum(): number { return this.x + this.y + this.w + this.z; }
    label(): string { return this.v ?? "none"; }
}
class Derived extends Base {
    r: number = 9;
    constructor(public q: number, private hidden: number) {
        const seed: number = q + 1;
        super(seed, 2);
        this.r = this.r + this.q + this.hidden;
    }
    total(): number { return this.sum() + this.r + this.hidden; }
}
const b = new Base(1, 2);
const d = new Derived(4, 5);
console.log(b.sum(), b.label(), d.total(), d.q, d.x);
console.log(Object.keys(b).join(","), Object.keys(d).join(","));
