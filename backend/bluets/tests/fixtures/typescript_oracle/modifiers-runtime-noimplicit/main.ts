// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Base { value: number = 1; read(): number { return this.value; } } export class Item extends Base { override value: number = 9; override read(): number { return super.read() + 1; } } console.log(new Item().read());
