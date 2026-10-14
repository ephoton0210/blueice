// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

const events: string[] = [];
function record(...args: any[]): void { events.push(typeof args[0] + ":" + String(args[1]) + ":" + typeof args[2]); }
const reflectAny: any = Reflect;
reflectAny.metadata = function (key: string, value: any): any { return function (target: any, member: any): void { events.push(key + ":" + (Array.isArray(value) ? value.map(function (item: any): any { return item.name; }).join(",") : value.name) + ":" + String(member)); }; };
@record export class Box { @record value: number = 42; @record answer(value: number): number { return value; } }
export const result: any[] = [new Box().answer(new Box().value), events];
