// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    static value: number = 0;
    static {
        let i: number = 0;
        while (i < 3) { A.value = A.value + i; i = i + 1; }
        if (A.value > 2) { A.value = A.value * 2; } else { A.value = 0; }
    }
}
const n: number = A.value;
