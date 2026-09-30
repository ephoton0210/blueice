// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Counter {
    #count: number = 0;
    static #made: number = 0;
    label: string = "c";
    #bump(step: number): number { this.#count = this.#count + step; return this.#count; }
    get #double(): number { return this.#count * 2; }
    set #double(value: number) { this.#count = value / 2; }
    static #make(): Counter { Counter.#made = Counter.#made + 1; return new Counter(); }
    run(other: Counter): number {
        this.#double = 8;
        return this.#bump(1) + other.#count + this.#double + Counter.#make().#count;
    }
    static made(): number { return Counter.#made; }
}
const c = new Counter();
const n: number = c.run(new Counter()) + Counter.made();
