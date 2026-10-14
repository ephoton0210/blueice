// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

export class Box<T> { constructor(private n: T) {} get value(): T { return this.n; } set value(value: T[]) { this.n = value[0]; } } const b = new Box(1); b.value = [7]; const n: number = b.value; console.log(n);
