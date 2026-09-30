// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    #secret: number = 1;
    constructor(public open: number, private hidden: number) {}
    run(): number { return this.#secret + this.open + this.hidden; }
}
