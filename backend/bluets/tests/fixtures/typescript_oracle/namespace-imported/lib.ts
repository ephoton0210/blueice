// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export interface Shape { sides: number }
export namespace Geo {
    export const origin: number = 0;
    export let counter: number = 1;
    export function area(s: Shape): number { return s.sides * 2; }
    export class Point { x: number = 1; dist(): number { return this.x + origin; } }
    export enum Kind { Flat, Round }
    export namespace Deep { export const depth: number = 3; export interface Tag { t: string } }
    export interface Box { shape: Shape }
    export type Id = number;
    interface Internal { i: number }
    export function internal(): Internal { return { i: 5 }; }
    export function bump(): number { counter += 1; return counter; }
}
export namespace Only { export interface T { v: number } }
