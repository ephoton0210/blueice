// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

namespace S {
    export const a: number = 1;
    export function f(): void {}
    export class K {}
    export enum E { X }
    export namespace In { export const z: number = 1; }
    export interface I { q: number }
    export type T = number;
}
console.log(Object.keys(S));
