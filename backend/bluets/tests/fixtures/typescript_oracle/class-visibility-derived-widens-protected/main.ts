// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    protected value: number = 1;
    protected read(): number { return this.value; }
}
class Derived extends Base {
    public value: number = 2;
    public read(): number { return 3; }
}
const d = new Derived();
const n: number = d.value + d.read();
