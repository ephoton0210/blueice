// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

class Box { private n: number = 1; get value(): number { return this.n; } set value(value: string) { this.n = value.length; } } export class Derived extends Box {} const b = new Derived(); b.value = "four"; console.log(b.value);
