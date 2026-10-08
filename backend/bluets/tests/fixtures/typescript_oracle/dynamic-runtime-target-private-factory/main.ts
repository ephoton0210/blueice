// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export {}; function make(value: number) { return class Inner { #value: number = value; static #count: number = 2; #read(): number { return this.#value; } read(): number { return this.#read() + Inner.#count; } }; } const A = make(3); const B = make(4); console.log(new A().read(), new B().read());
