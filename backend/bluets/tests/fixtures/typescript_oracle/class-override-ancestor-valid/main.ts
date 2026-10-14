// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Grand {
    read(value: number): number { return value; }
    static parse(value: number): number { return value; }
}
class Middle extends Grand {}
class Child extends Middle {
    read(value: number): number { return value; }
    static parse(value: number): number { return value; }
}
declare const child: Child;
const value: number = child.read(1);
const parsed: number = Child.parse(2);
