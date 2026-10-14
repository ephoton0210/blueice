// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class Counter {
    count: number = 1;
    static total: number = 10;
    label = "c";
    readonly id: number = 7;
    tag?: string;
    name!: string;
    constructor() { this.name = "n"; }
    step(): number { this.count = this.count + 1; return this.count; }
}
class Sub extends Counter {
    extra: number = 5;
    static made: number = Counter.total + 1;
}
const c = new Sub();
console.log(c.step(), c.label, c.id, c.tag === undefined, c.name, c.extra, Sub.made, Counter.total);
console.log(Object.keys(c).join(","));
