// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box { private n: number = 1; get value(): number { return this.n; } set value(value: string) { this.n = value.length; } } const b = new Box(); b.value = "four"; console.log(b.value);
