// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export class A {
    private _v: number = 1;
    get v(): number { return this._v; }
    set v(x: number) { this._v = x; }
    get ro(): string { return "r"; }
    set wo(x: number) {}
    static get count(): number { return 1; }
    static set count(x: number) {}
    protected get p(): number { return 1; }
    private get hidden(): number { return 1; }
    private set hidden(x: number) {}
    set both(x: number) {}
    get both(): number { return 1; }
}
