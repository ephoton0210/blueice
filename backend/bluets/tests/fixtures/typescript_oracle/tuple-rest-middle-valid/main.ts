// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export type Packet = [head: number, ...body: string[], done: boolean];
export const short: [head: number, ...body: string[], done: boolean] = [1, true];
export const long: [head: number, ...body: string[], done: boolean] = [1, 'a', 'b', true];
export const leading: [...names: string[], enabled: boolean] = [false];
export const leadingMany: [...names: string[], enabled: boolean] = ['a', 'b', true];
const head: number = long[0];
const uncertain: string | boolean = long[1];
const far: string | boolean = long[10];
export const renamed: [start: number, ...middle: string[], end: boolean] = short;
export const fixed: [number, string, boolean] = [1, 'a', true];
export const fromFixed: [number, ...string[], boolean] = fixed;
export const allStrings: [...string[], string] = ['a', 'b'];
export const array: string[] = allStrings;
export const trailingStrings: [...string[]] = allStrings;
