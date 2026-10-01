// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace N {
    export class A { a: number = 1; }
    export class B extends A { b: number = 2; }
}
const b: N.B = new N.B();
const n: number = b.a + b.b;
console.log(n);
