// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

let reads = 0;
function source(): number[] { reads++; return [20, 22]; }
export function sum(...values: number[]): number {
    return values.reduce((left, right) => left + right, 0);
}
const box = { first(...values: number[]): number { return values[0]; } };
export const answer: number = sum(...source());
export const sourceReads: number = reads;
export const member: number = box.first(...[42]);
export const arity: number = sum.length;
