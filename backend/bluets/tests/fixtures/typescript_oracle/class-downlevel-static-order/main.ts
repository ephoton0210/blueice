// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class A {
    static log: string[] = [];
    static a: number = A.note("a", 1);
    static f: () => number = () => this.a;
    static obj: { m(): string; arrow: () => number } = { m(): string { return typeof this; }, arrow: (): number => this.a };
    static { A.note("b1", 0); }
    static b: number = A.note("b", 2);
    static {
        const local: number = this.a + this.b;
        A.log.push("local" + local);
    }
    instance: number = A.note("instance", 3);
    static note(label: string, value: number): number { A.log.push(label); return value; }
}
class B extends A {
    static c: number = A.a + 100;
    static { B.log.push("child" + this.c); }
    other: number = A.note("other", 4);
}
const before: string = A.log.join(",");
new B();
console.log(before);
console.log(A.log.join(","), A.f(), A.obj.m(), A.obj.arrow(), Object.keys(A).join(","));
