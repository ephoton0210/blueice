// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(value: number): number { return value; }
    flexible(value: number | string): number { return 1; }
    static parse(value: number): number { return value; }
    static flexibleParse(value: number | string): number { return 1; }
}
class Child extends Base {
    read(value: number): number { return value; }
    flexible(value: number): number { return value; }
    static parse(value: number): number { return value; }
    static flexibleParse(value: number): number { return value; }
}
declare const child: Child;
const value: number = child.read(1);
const flexible: number = child.flexible(1);
const parsed: number = Child.parse(2);
const flexibleParsed: number = Child.flexibleParse(2);
