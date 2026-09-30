// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box {
    private _v: number = 1;
    protected get inner(): number { return this._v; }
    protected set inner(value: number) { this._v = value; }
    private get hidden(): number { return this._v; }
    public get open(): number { return this.hidden + this.inner; }
}
class Sub extends Box {
    twice(): number { return this.inner * 2; }
}
const n: number = new Sub().open + new Sub().twice();
