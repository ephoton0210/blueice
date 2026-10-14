// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
type Prefix<T extends string[]> = [number, ...T];
export function same<T extends string[]>(value: Prefix<T>): [number, ...T] { return value; }
export function widen<T extends [string]>(value: [number, ...T]): [number, ...string[]] { return value; }
export function local<T extends string[]>(value: [number, ...T]): void { const copy: Prefix<T> = value; }
