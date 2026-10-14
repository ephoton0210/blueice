// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export type Trail = [head: number, ...tail: string[]];
export const empty: [head: number, ...tail: string[]] = [1];
export const many: [head: number, ...tail: string[]] = [1, 'a', 'b'];
const first: number = many[0];
const later: string = many[2];
export const spread: [...number[]] = [1, 2];
export const array: number[] = spread;
export const fromArray: [...number[]] = array;
export const optional: [head?: number, ...tail: string[]] = [];
export const optionalFilled: [head?: number, ...tail: string[]] = [1, 'a'];
export const optionalFromArray: [head?: number, ...tail: number[]] = array;
