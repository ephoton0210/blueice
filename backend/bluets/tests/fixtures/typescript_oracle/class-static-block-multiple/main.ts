// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    static x: number = 1;
    static { A.x = A.x + 1; }
    static { A.x = A.x * 10; }
}
const n: number = A.x;
