// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
function f(a: number, b?: string): number { return a; }
function g(a: number, b?: string, c?: boolean): number { return a; }
export function run(t: [number, string?], u: [number, string?, boolean?]): number { return f(...t) + g(...u); }
const one: [string?] = [];
export const w: [number, string?] = [1, ...one];
export const x: [number, string?, boolean?] = [1, ...one];
class A { constructor(a: number, b?: string) {} m(a: number, b?: string): number { return a; } }
export function make(t: [number, string?]): A { return new A(...t); }
export function call(o: A, t: [number, string?]): number { return o.m(...t); }
