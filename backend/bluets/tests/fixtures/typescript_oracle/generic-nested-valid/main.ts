// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export const a = <T>(value: T): T => value;
export const b = <T extends { n: number }>(v: T): number => v.n;
export const c = function <T, U>(x: T, f: (t: T) => U): U { return f(x); };
export function outer(): number { function d<T>(xs: T[]): T { return xs[0]; } return d([1]); }
export const e = <T,>(value: T): T => value;
