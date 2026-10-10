// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

declare const console: { log(first?: unknown, second?: unknown, third?: unknown): void };
class Base { value = 20; getValue(): number { return this.value; } }
class Derived extends Base { other = 22; sum(): number { return this.getValue() + this.other; } }
export const result = new Derived().sum();
console.log(result);
