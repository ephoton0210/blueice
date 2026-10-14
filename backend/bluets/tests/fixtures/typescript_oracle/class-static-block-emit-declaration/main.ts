// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export class A {
    static count: number = 0;
    static {
        A.count = 1;
    }
    a: number = 1;
    static {
        A.count = 2;
    }
}
