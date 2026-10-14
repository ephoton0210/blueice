// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    constructor(public id: number) {}
}
class Derived extends Base {
    constructor(public extra: number, flag: boolean) {
        if (flag) { super(1); } else { super(2); }
    }
}
