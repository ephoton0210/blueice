// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class Temperature {
    private _celsius: number = 0;
    static made: number = 0;
    get celsius(): number { return this._celsius; }
    set celsius(value: number) { this._celsius = value; }
    get fahrenheit(): number { return this._celsius * 2 + 32; }
    set kelvin(value: number) { this._celsius = value - 273; }
    static get unit(): string { Temperature.made = Temperature.made + 1; return "C" + Temperature.made; }
    constructor(start: number) { this.celsius = start; }
}
class Warm extends Temperature {
    get celsius(): number { return super.celsius + 1; }
    set celsius(value: number) { super.celsius = value; }
}
const t = new Temperature(10);
t.celsius = t.celsius + 5;
t.kelvin = 300;
const w = new Warm(20);
console.log(t.celsius, t.fahrenheit, Temperature.unit, Temperature.unit, w.celsius, Object.keys(t).join(","));
console.log(Object.getOwnPropertyNames(Temperature.prototype).join(","));
