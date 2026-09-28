// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
type Prefix<T extends string[]> = [number, ...T];
export function consume<T extends string[]>(value: Prefix<T>): void {}
export function direct<T extends string[]>(value: [number, ...T]): void {}
export function fixed<T extends [string, boolean]>(value: [number, ...T]): void {}
