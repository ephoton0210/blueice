// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const events: string[] = [];
function observe(value: any, context: any): void { events.push(context.kind + ":" + String(context.name) + ":" + context.static + ":" + context.private); }
export class Box { value: number = 0; @observe set #answer(value: number) { this.value = value; } write(value: number): void { this.#answer = value; } }
const box = new Box(); box.write(42);
export const result: any[] = [box.value, events];
