// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    static parse(value: string): string;
    static parse(value: number): number;
    static parse(value: any): any { return value; }
}
class Middle extends Base {}
class Child extends Middle { static fromThis(): number { return this.parse(1); } }
const text: string = Child.parse('ok');
const numberValue: number = Child.parse(2);
