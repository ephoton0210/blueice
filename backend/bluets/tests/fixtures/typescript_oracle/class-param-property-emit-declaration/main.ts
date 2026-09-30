// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export class Base {
    z: number = 5;
    constructor(public x: number, private y: string, protected readonly w: number = 3, readonly v?: string, public flag = true) {}
    m(): number { return this.x; }
}
export class Derived extends Base {
    constructor(public q: number, protected r?: number) { super(1, "a"); }
}
export class Plain {
    constructor(a: number, b?: string) {}
}
