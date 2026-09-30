// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box {
    protected static tag: string = "t";
}
class Sub extends Box {
    static viaSelf(): string { return Sub.tag; }
}
class Sibling extends Box {
    static viaBase(): string { return Box.tag + Sub.tag; }
    static viaOther(): string { return Sub.tag; }
}
const s: string = Sub.viaSelf() + Sibling.viaBase();
