// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const events: string[] = [];
function record(...args: any[]): void { events.push(typeof args[0] + ":" + String(args[1]) + ":" + typeof args[2]); }
export class Box { constructor(@record public value: number) {} }
export const result: any[] = [new Box(42).value, events];
