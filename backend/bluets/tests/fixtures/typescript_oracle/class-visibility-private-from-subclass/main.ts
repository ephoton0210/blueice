// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    private secret: number = 1;
}
class Derived extends Base {
    read(): number { return this.secret; }
}
