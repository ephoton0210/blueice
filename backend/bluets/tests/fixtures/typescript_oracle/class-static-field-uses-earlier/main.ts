// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    static a: number = 1;
    static b: number = A.a + 1;
    static c: number = this.b + this.a;
}
const n: number = A.c;
