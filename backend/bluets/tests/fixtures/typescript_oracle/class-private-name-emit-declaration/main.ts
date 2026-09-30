// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
export class A {
    a: number = 1;
    #x: number = 1;
    b(): number { return 1; }
    #m(): void {}
    get #g(): number { return 1; }
    static #s = 1;
    c: number = 2;
}
export class B extends A {
    d: number = 3;
}
export class C extends A {
    #y = 1;
    static #z = 1;
}
export class D {
    static #only = 1;
    e: number = 1;
    private p: number = 1;
}
export class E {
    #q: number = 1;
    constructor(public open: number) {}
}
