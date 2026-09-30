// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Counter {
    private static _n: number = 0;
    static get n(): number { return Counter._n; }
    static set n(value: number) { Counter._n = value; }
}
Counter.n = 3;
const n: number = Counter.n;
