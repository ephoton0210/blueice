// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export abstract class Base { protected constructor(public readonly value: number) {} abstract read(): number; } export class Item extends Base { constructor() { super(5); } override read(): number { return this.value; } } export type Ctor = abstract new () => Base; console.log(new Item().read());
