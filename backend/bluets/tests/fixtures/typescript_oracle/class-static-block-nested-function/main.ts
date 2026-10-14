// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    static value: number = 0;
    static {
        function bump(step: number): number { return step + 1; }
        const f = (x: number): number => x * 2;
        A.value = bump(1) + f(3);
    }
}
const n: number = A.value;
