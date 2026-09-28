// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Base {
    read(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }
    prefix(...parts: [head: number, extra: string, ...middle: string[], done: boolean]): number { return 1; }
    suffix(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }
    middle(...parts: [head: number, ...middle: string[], end: string]): number { return 1; }
    trailing(...parts: [head: number, ...tail: string[]]): number { return 1; }
    static parse(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }
}
class Child extends Base {
    read(...parts: [first: number, ...body: string[], last: boolean]): number { return 1; }
    prefix(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }
    suffix(...parts: [head: number, ...middle: string[], extra: string, done: boolean]): number { return 1; }
    middle(...parts: [head: number, ...tail: string[]]): number { return 1; }
    trailing(...parts: [head: number, ...middle: string[], end: string]): number { return 1; }
    static parse(...parts: [head: number, ...middle: string[], done: boolean]): number { return 1; }
}
