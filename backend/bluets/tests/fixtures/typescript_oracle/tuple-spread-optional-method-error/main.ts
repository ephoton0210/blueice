// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class A { m(a: number, b: string): number { return a; } }
export function run(o: A, t: [number, string?]): number { return o.m(...t); }
