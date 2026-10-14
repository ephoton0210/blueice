// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const events: string[] = [];
function note(value: string): void { events.push(value); }
function observe(value: any, context: any): void { note(context.kind + ":" + String(context.name) + ":" + context.static + ":" + context.private); }
function increase(value: any, context: any): any { note(context.kind + ":" + String(context.name)); return function (initial: number): number { return initial + 1; }; }
function wrap(value: any, context: any): any { note(context.kind + ":" + String(context.name)); return function (this: any, ...args: any[]): any { return value.apply(this, args) + 1; }; }
class Base { static answer(): number { return 41; } } @observe export class Box extends Base { @observe static answer(): number { return super.answer() + 1; } }
export const result: any[] = [Box.answer(), events];
