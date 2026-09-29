// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class Shape {
  constructor(name: string);
  constructor(name: string, sides: number);
  constructor(name: string, sides?: number) { console.log('shape ' + name); }
  describe(prefix: string): string { return prefix + 'shape'; }
  scale(factor: number): number;
  scale(factor: string): string;
  scale(factor: any): any { return factor; }
  static create(name: string): string { return 'made ' + name; }
}
class Square extends Shape {
  constructor() { super('square', 4); console.log('square'); }
  describe(prefix: string): string { return super.describe(prefix) + ' square'; }
  static create(name: string): string { return super.create(name) + '!'; }
}
const square = new Square();
console.log(square.describe('a '));
console.log(square.scale(2));
console.log(Square.create('x'));
