// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    constructor(value: string);
    constructor(value: number);
    constructor(value: any) {}
    label(): string { return 'base'; }
}
class Middle extends Base {}
class Child extends Middle {}
const numberChild: Child = new Child(1);
const stringChild: Child = new Child('ok');
