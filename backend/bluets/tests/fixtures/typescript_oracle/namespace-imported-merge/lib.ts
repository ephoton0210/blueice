// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class K {
    static s: number = 1;
    v: number = 2;
}
export namespace K {
    export const t: number = 3;
    export function make(): K { return new K(); }
}
export enum E { A, B }
export namespace E {
    export function label(e: E): string { return e === E.A ? "a" : "b"; }
}
export function g(x: number): number { return x + 1; }
export namespace g {
    export const meta: string = "m";
}
