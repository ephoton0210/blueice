// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Temp {
    private _c: number = 1;
    unit: string = "C";
    get c(): number { return this._c; }
    set c(v: number) { this._c = v; }
    static made: number = 0;
    static make(): Temp { Temp.made = Temp.made + 1; return new Temp(); }
    scale(k: number): number { return this._c * k; }
}
const t = Temp.make();
t.c = 4;
console.log(t.scale(3), t.unit, Temp.made, Object.keys(t).join(","), Object.getOwnPropertyNames(Temp.prototype).join(","));
