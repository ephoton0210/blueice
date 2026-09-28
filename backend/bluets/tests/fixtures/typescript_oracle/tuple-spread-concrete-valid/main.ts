// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export type Pair = [number, string];
export type WithHead = [boolean, ...Pair];
export type WithTail = [...WithHead, null];
export type Maybe = [number, string?];
export type OptionalPrefix = [number?];
export type RequireTail = [...OptionalPrefix, string];
export const direct: [...Pair] = [1, "one"];
export const middle: WithHead = [true, 2, "two"];
export const nested: WithTail = [false, 3, "three", null];
export const optional: [...Maybe] = [4];
export const requiredAfterOptional: RequireTail = [undefined, "tail"];
export const first: number = direct[0];
export const second: string = middle[2];
