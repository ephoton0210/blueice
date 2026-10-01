// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export namespace N {
    export const a: number = 1;
    export let b: number = 2;
    const hidden: number = 5;
    export function f(x: number): number { return x + hidden; }
    export class C { v: number = 1; static s: string = "s"; m(): number { return 1; } }
    export enum E { X, Y = 5 }
    export namespace Inner { export const z: number = 3; export interface J { q: number } }
    export interface I { q: number }
    export type T = string;
    interface Hidden { h: number }
    export function usesHidden(): Hidden { return { h: 1 }; }
}
export namespace A.B.C { export const k: string = "k"; }
namespace Local { export const l: number = 1; }
export { Local };
export namespace Types { export interface P { x: number } }
export namespace Ambient { export interface Z { z: number } }

export namespace Plain { export const only: string = "o"; }
console.log(N.a, N.f(1), new N.C().m(), N.E.Y, N.Inner.z, A.B.C.k, Local.l, Plain.only);
