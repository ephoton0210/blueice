// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    x: number = 1;
    y = "s";
    z?: number;
    w!: number;
    static count: number = 0;
    static label: string = "L" + Base.count;
    static tagged: string = String(this.count);
    static {
        Base.count = 5;
        this.label = "B" + this.count;
    }
    constructor(public p: number, private q: number = 2) {
        console.log("ctor", this.x, this.p, this.q);
    }
    show(): string { return [this.x, this.y, this.z, this.p, this.q].join("|"); }
}
class Derived extends Base {
    w2: number = this.x + 10;
    static s2: number = Base.count + 1;
    constructor() {
        super(1);
        console.log("derived", this.w2);
    }
}
class NoCtor extends Base {
    e: number = 5;
}
class Plain {
    a = 1;
    b: number = this.a + 1;
}
const d = new Derived();
console.log(Object.keys(d).join(","), Object.keys(new Plain()).join(","), new NoCtor(9).show());
console.log(Base.label, Base.count, Base.tagged, Derived.s2, Object.keys(Base).join(","));
console.log(d.show(), JSON.stringify(new Plain()), "z" in d, "w" in d, "q" in d);
