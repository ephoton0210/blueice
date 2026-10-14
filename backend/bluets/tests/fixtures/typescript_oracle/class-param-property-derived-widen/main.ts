// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    constructor(protected a: number) {}
}
class Derived extends Base {
    constructor(public a: number) { super(a); }
}
const n: number = new Derived(1).a;
