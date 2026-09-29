// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export class Shape {
  constructor(name: string);
  constructor(name: string, sides: number);
  constructor(name: string, sides?: number) { console.log(name); }
  describe(prefix: string): string { return prefix + 'shape'; }
  scale(factor: number): number;
  scale(factor: string): string;
  scale(factor: any): any { return factor; }
  static create(name: string): string { return 'made ' + name; }
}
export class Square extends Shape {
  constructor() { super('square', 4); }
  describe(prefix: string): string { return super.describe(prefix) + ' square'; }
}
class Hidden { constructor() {} }
