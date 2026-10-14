// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    private _v: number = 1;
    get v(): number { return this._v; }
    set v(value: number) { this._v = value; }
    constructor(start: number) { this.v = start; }
}
const n: number = new A(5).v;
