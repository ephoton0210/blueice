// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    short(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }
    long(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }
    collect(...parts: [head: number, ...middle: string[], end: string]): number { return 1; }
    leading(...parts: [...middle: string[], end: string]): number { return 1; }
    shifted(a: number, ...parts: [...middle: string[], done: boolean]): number { return a; }
    static parse(...parts: [head: number, ...middle: string[], end: string]): number { return 1; }
}
class Child extends Base {
    short(a: number, b: boolean): number { return a; }
    long(a: number, b: string, c: boolean): number { return a; }
    collect(a: number, ...parts: string[]): number { return a; }
    leading(...parts: string[]): number { return 1; }
    shifted(a: number, b: boolean): number { return a; }
    static parse(a: number, ...parts: string[]): number { return a; }
}
