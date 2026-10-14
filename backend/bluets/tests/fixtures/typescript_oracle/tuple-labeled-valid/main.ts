// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export type Pair = [first: number, second?: string];
export const one: [first: number, second?: string] = [1];
export const two: [left: number, right?: string] = one;
const first: number = one[0];
const second: string | undefined = one[1];
