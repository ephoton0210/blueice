// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function take(p: [number, string]): number { return p[0]; }
function optional(p: [number, string?]): number { return p[0]; }
function nested(p: [[number, string], boolean]): boolean { return p[1]; }
function over(p: [number]): number;
function over(p: [string, string]): string;
function over(p: any): any { return p; }
type Pair = [number, string];
function named(p: Pair): number { return p[0]; }
export const a: number = take([1, "a"]) + optional([1]) + optional([1, "b"]);
export const b: boolean = nested([[1, "a"], true]);
export const c: number = over([1]);
export const d: string = over(["a", "b"]);
export const e: number = named([2, "z"]);
const local = 3;
export const f: number = take([local, "l"]);
