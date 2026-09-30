// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base { label(value: number): string { return 'base'; } kind(): string | number { return 1; } }
class Middle extends Base {}
class Child extends Middle { read(): string { return this.label(1); } kind(): string { return 'child'; } }
const child = new Child();
const text: string = child.label(2);
const kind: string = child.kind();
