// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const events: string[] = [];
function observe(value: any, context: any): void { events.push(context.kind + ":" + String(context.name) + ":" + context.static + ":" + context.private); }
function adjust(value: any, context: any): any { observe(value, context); return {get: function(this: any): number { return value.get.call(this) + 1; }, init: function(value: number): number { return value; }}; }
export class Box { @adjust accessor #value: number = 41; answer(): number { return this.#value; } }
export const result: any[] = [new Box().answer(), events];
