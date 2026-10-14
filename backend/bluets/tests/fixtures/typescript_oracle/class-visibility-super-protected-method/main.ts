// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    protected who(): string { return "base"; }
    protected static tag(): string { return "T"; }
}
class Derived extends Base {
    protected who(): string { return super.who() + "!"; }
    static both(): string { return super.tag() + Derived.tag(); }
    say(): string { return this.who(); }
}
const s: string = new Derived().say() + Derived.both();
