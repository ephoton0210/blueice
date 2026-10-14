// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Order {
    static log: string[] = [];
    static a: number = Order.note("a", 1);
    static {
        Order.note("block1", 0);
    }
    static b: number = Order.note("b", 2);
    static {
        Order.note("block2", 0);
    }
    instance: number = Order.note("instance", 3);
    static note(label: string, value: number): number { Order.log.push(label); return value; }
}
const before: string = Order.log.join(",");
new Order();
const after: string = Order.log.join(",");
