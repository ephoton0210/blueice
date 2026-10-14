// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    value: number = 1;
    static shared: number = 2;
}
class Derived extends Base {
    doubled(): number { return this.value * 2; }
    static shared2(): number { return this.shared; }
}
const d = new Derived();
const n: number = d.value;
