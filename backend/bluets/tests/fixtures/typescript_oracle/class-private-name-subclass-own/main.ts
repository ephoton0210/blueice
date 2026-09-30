// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    #x: number = 1;
    baseX(): number { return this.#x; }
}
class Derived extends Base {
    #x: string = "s";
    derivedX(): string { return this.#x; }
}
const d = new Derived();
const n: number = d.baseX();
const s: string = d.derivedX();
