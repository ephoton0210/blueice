// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    get v(): number { return 1; }
    set v(value: number) {}
}
class Derived extends Base {
    get v(): number { return 2; }
    set v(value: number) {}
}
const n: number = new Derived().v;
