// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    get v(): number { return 1; }
}
const x: { v: number } = new A();
const y: { readonly v: number } = new A();
const n: number = x.v + y.v;
