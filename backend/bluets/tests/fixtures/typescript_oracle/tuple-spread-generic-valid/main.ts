// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export type Prefix<T extends unknown[]> = [number, ...T];
export type Concrete = Prefix<[string, boolean]>;
export type Tail<T extends string[] = string[]> = [boolean, ...T, number];
export type Exact<T extends [string, boolean]> = [number, ...T];
export type Wrapped<T extends unknown[]> = [...Prefix<T>, boolean];
export const first: Concrete = [1, "one", true];
export const second: Prefix<[string, boolean]> = [2, "two", false];
export const empty: Tail<[]> = [false, 3];
export const many: Tail<string[]> = [true, "a", "b", 4];
export const defaulted: Tail = [true, "c", 5];
export const exact: Exact<[string, boolean]> = [6, "six", true];
export const wrapped: Wrapped<[string]> = [7, "seven", false];
