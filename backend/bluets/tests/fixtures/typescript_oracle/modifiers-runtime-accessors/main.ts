// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export abstract class Base { abstract get value(): number; abstract set value(v: number); } export class Item extends Base { private stored: number = 2; override get value(): number { return this.stored; } override set value(v: number) { this.stored = v; } } const item = new Item(); item.value = 7; console.log(item.value);
