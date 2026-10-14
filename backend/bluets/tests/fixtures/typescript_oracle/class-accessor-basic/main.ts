// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Temperature {
    private _celsius: number = 0;
    get celsius(): number { return this._celsius; }
    set celsius(value: number) { this._celsius = value; }
    get fahrenheit(): number { return this._celsius * 2 + 32; }
    set kelvin(value: number) { this._celsius = value - 273; }
    static get unit(): string { return "C"; }
    inc(): number { this.celsius = this.celsius + 1; return this.celsius; }
}
const t = new Temperature();
t.celsius = 20;
t.kelvin = 300;
const n: number = t.celsius + t.fahrenheit + t.inc();
const u: string = Temperature.unit;
