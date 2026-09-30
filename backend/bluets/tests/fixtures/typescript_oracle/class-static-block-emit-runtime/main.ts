// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
class Order {
    static log: string[] = [];
    static a: number = Order.note("a", 1);
    static {
        Order.note("block1", 0);
        const local: number = this.a + 10;
        Order.log.push("local" + local);
    }
    static b: number = Order.note("b", 2);
    static #hidden: number = Order.note("hidden", 3);
    static {
        Order.note("block2", Order.#hidden + this.b);
    }
    instance: number = Order.note("instance", 4);
    static note(label: string, value: number): number { Order.log.push(label); return value; }
}
class Child extends Order {
    static extra: number = Order.a + 1;
    static {
        Order.log.push("child" + this.extra + super.a);
    }
}
const before: string = Order.log.join(",");
new Order();
new Child();
console.log(before);
console.log(Order.log.join(","));
