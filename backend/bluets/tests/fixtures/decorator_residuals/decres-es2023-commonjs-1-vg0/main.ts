// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const events: string[] = [];
function observe(value: any, context: any): void { events.push(context.kind + ":" + String(context.name) + ":" + context.static + ":" + context.private); }
function replace(value: any, context: any): any { observe(value, context); return class extends value { static factor: number = 2; }; }
class Base { static factor: number = 1; static saved: number = 0; static get answer(): number { return this.factor * 20; } static set answer(value: number) { this.saved = value + 1; } }
@replace export class Box extends Base { static get value(): number { return super.answer + 2; } }
export const result: any[] = [Box.value, events];
