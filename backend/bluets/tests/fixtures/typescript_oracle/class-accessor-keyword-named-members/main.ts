// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    get: number = 1;
    set(): number { return 2; }
    static: number = 3;
    get value(): number { return this.get + this.set() + this.static; }
    set value(input: number) { this.get = input; }
}
const a = new A();
a.value = 5;
const n: number = a.value;
