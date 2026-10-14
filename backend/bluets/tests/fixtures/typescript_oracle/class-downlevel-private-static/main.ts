// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Registry {
    static #items: string[] = [];
    static #size: number = 0;
    static #secret: number = Registry.#compute(3);
    static #compute(n: number): number { return n * 7; }
    static get #total(): number { return Registry.#size + Registry.#secret; }
    static {
        Registry.#items.push("boot");
        Registry.#size = Registry.#items.length;
    }
    static add(item: string): number {
        Registry.#items.push(item);
        Registry.#size += 1;
        return Registry.#total;
    }
    static list(): string { return Registry.#items.join(","); }
}
console.log(Registry.add("a"), Registry.add("b"), Registry.list());
