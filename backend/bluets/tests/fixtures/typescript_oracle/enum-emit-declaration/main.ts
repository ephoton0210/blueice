// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

function seed(): number { return 3; }
export enum Num { A, B = 5, C, D = A | 8, Neg = -1, F = 1.5 }
export enum Str { A = "a", B = `t`, "q-r" = "x" }
export enum Mixed { A = 1, B = "b", C = 3 }
export enum Computed { A = seed(), B = 2 }
export const enum Konst { X = 1, Y = "y" }
export const enum Auto { A, B, C = 10, D }
export declare enum Amb { A, B = 2, C }
export declare const enum AmbConst { A, B }
export enum Merged { A }
export enum Merged { B = 1 }
enum Local { A, B }
export enum Empty {}
export enum Big { A = 2 ** 40, B = 1e21, Inf = 1 / 0, NaN2 = 0 / 0 }
export { Local };
enum Private { A }
export class Holder { kind: Num = Num.A; static mode: Str = Str.A; pick(k: Konst): Mixed { return Mixed.A; } }
console.log(Num.A);
