// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    constructor(public id: number) {}
}
class Derived extends Base {
    constructor(public extra: number) {
        const seed: number = extra + 1;
        super(seed);
    }
}
const d = new Derived(2);
const n: number = d.id + d.extra;
