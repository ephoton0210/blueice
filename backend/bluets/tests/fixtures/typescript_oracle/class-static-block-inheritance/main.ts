// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    static seed: number = 10;
    static baseLog: number[] = [];
    static {
        Base.baseLog.push(Base.seed);
    }
}
class Derived extends Base {
    static extra: number = Base.seed + 1;
    static {
        Derived.baseLog.push(this.extra);
        Derived.baseLog.push(super.seed);
    }
}
const n: number = Derived.baseLog.length;
