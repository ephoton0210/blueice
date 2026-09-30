// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    private secret: number = 1;
    read(): number { return this.secret; }
}
class B extends A {}
const a: A = new B();
const n: number = a.read();
