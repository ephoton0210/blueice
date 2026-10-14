// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Registry {
    static count: number = 0;
    static names: string[] = [];
    static #secret: number = 7;
    static {
        Registry.count = Registry.#secret * 2;
        this.names.push("boot");
        const local: number = this.count + 1;
        this.count = local;
    }
    static total(): number { return Registry.count + Registry.names.length; }
}
const n: number = Registry.total();
