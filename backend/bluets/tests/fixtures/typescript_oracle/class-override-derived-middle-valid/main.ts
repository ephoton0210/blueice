// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(a: number, b: string, c: boolean): number { return a; }
    leading(a: number): number { return a; }
    collect(a: number, ...parts: string[]): number { return a; }
    static parse(a: number, ...parts: string[]): number { return a; }
}
class Child extends Base {
    read(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }
    leading(...parts: [...middle: string[], done: number]): number { return 1; }
    collect(...parts: [head: number, ...middle: string[], end: string]): number { return 1; }
    static parse(a: number, ...parts: [...middle: string[], end: string]): number { return a; }
}
