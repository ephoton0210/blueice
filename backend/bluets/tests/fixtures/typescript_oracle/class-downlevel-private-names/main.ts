// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Counter {
    #count: number = 0;
    #label: string;
    #hook: (n: number) => number = (n: number): number => n + 1;
    static #made: number = 0;
    pub: string = "p";
    constructor(label: string) {
        this.#label = label;
        Counter.#made = Counter.#made + 1;
    }
    #bump(step: number): number { this.#count = this.#count + step; return this.#count; }
    get #double(): number { return this.#count * 2; }
    set #double(value: number) { this.#count = value / 2; }
    static #make(label: string): Counter { return new Counter(label); }
    static create(label: string): Counter { return Counter.#make(label); }
    run(other: Counter): string {
        this.#count += 2;
        this.#count *= 3;
        this.#count++;
        --other.#count;
        const a: number = this.#bump(3) + other.#count + this.#double + this.#hook(1);
        this.#double = a;
        this.#label = "z" + this.#label;
        return [this.#label, this.#count, a, Counter.#made, this.pub].join("|");
    }
    static has(value: any): boolean { return #count in value; }
    hasMethod(value: any): boolean { return #bump in value; }
    static get made(): number { return Counter.#made; }
}
class Sub extends Counter {
    #count: number = 100;
    sub(): number { return this.#count; }
}
const c = Counter.create("q");
console.log(c.run(Counter.create("r")), Counter.has(c), Counter.has({}), new Sub("s").sub(), Counter.has(new Sub("t")), c.hasMethod(c), Counter.made);
console.log(Object.keys(c).join(","), JSON.stringify(c));
