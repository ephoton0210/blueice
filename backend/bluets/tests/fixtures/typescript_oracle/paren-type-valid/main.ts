// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export function f(v: (string | number)[]): void {}
export function g(v: [boolean, ...(string | number)[]]): void {}
export function h(v: ((a: number) => string)[]): void {}
export function k(v: (string | number)): (string | number) { return v; }
export type Mixed = (string | number)[];
export function m(v: Mixed): void {}
