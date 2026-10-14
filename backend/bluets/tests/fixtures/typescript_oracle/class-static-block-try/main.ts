// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    static value: number = 0;
    static {
        try { A.value = 1; } catch (e) { A.value = 2; } finally { A.value = A.value + 10; }
    }
}
const n: number = A.value;
