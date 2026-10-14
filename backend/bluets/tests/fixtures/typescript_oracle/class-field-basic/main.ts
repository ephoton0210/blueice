// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Counter {
    count: number = 0;
    static total: number = 10;
    label = "counter";
    readonly id: number = 7;
    tag?: string;
    name!: string;
    increment(): number {
        this.count = this.count + 1;
        return this.count;
    }
    static bump(): number {
        this.total = this.total + 1;
        return this.total;
    }
}
const counter = new Counter();
const next: number = counter.increment();
const total: number = Counter.bump();
const label: string = counter.label;
const id: number = counter.id;
