// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    protected static shared: number = 1;
    private static hidden: number = 2;
    static read(): number { return Derived.hidden + Derived.shared; }
}
class Derived extends Base {
    static peek(): number { return Derived.shared + Base.shared; }
}
